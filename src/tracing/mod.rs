use crate::{
    opcode::immediate_size,
    tracing::{
        arena::PushTraceKind,
        types::{
            CallKind, CallTraceNode, RecordedMemory, StepDelta, StorageChange, StorageChangeReason,
            TraceMemberOrder,
        },
        utils::gas_used,
    },
};
use alloc::{boxed::Box, vec::Vec};
use core::{borrow::Borrow, mem, ops::Range};
use revm::{
    bytecode::opcode::{self, OpCode},
    context::{JournalTr, LocalContextTr},
    context_interface::{Cfg, ContextTr},
    inspector::JournalExt,
    interpreter::{
        interpreter_types::{Immediates, Jumps, LoopControl, ReturnData, RuntimeFlag},
        CallInput, CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome, Interpreter,
        InterpreterResult,
    },
    primitives::{hardfork::SpecId, Address, Bytes, Log, B256, U256},
    Inspector, JournalEntry,
};

mod arena;
pub use arena::CallTraceArena;

mod builder;
pub use builder::{
    geth::{self, GethTraceBuilder},
    parity::{self, ParityTraceBuilder},
};

mod config;
pub use config::{OpcodeFilter, StackSnapshotType, TracingInspectorConfig};

mod limits;
use limits::TraceBudget;
pub use limits::{TraceError, TraceLimits};

mod fourbyte;
pub use fourbyte::FourByteInspector;

mod opcount;
pub use opcount::OpcodeCountInspector;

pub mod types;
use types::{CallLog, CallTrace, CallTraceStep};

mod utils;

#[cfg(feature = "std")]
mod writer;
#[cfg(feature = "std")]
pub use writer::{TraceWriter, TraceWriterConfig};

#[cfg(feature = "js-tracer")]
pub mod js;

mod mux;
pub use mux::{Error as MuxError, MuxInspector};

mod debug;
pub use debug::{DebugInspector, DebugInspectorError};

/// An inspector that collects call traces.
///
/// This [Inspector] can be hooked into revm's EVM which then calls the inspector
/// functions, such as [Inspector::call] or [Inspector::call_end].
///
/// The [TracingInspector] keeps track of everything by:
///   1. start tracking steps/calls on [Inspector::step] and [Inspector::call]
///   2. complete steps/calls on [Inspector::step_end] and [Inspector::call_end]
#[derive(Clone, Debug, Default)]
pub struct TracingInspector {
    /// Configures what and how the inspector records traces.
    config: TracingInspectorConfig,
    /// Recording budget and any sticky limit error.
    budget: TraceBudget,
    /// Records all call traces
    traces: CallTraceArena,
    /// Tracks active calls
    trace_stack: Vec<usize>,
    /// Tracks whether the next `step_end` should be recorded. Set in `start_step`.
    record_step_end: bool,
    /// Number of logs recorded so far, used as the index of the next log.
    log_count: usize,
    /// Number of opcode steps captured across all calls since the last reset.
    recorded_steps: u64,
    /// Tracks the journal len in the step, used in step_end to check if the journal has changed
    last_journal_len: usize,
    /// The spec id of the EVM.
    ///
    /// This is filled during execution.
    spec_id: Option<SpecId>,
    /// Pool of reusable _empty_ step vectors to reduce allocations.
    ///
    /// All `Vec<CallTraceStep>` are always empty but may have capacity.
    reusable_step_vecs: Vec<Vec<CallTraceStep>>,
}

impl TracingInspector {
    /// Returns a new instance for the given config
    pub fn new(config: TracingInspectorConfig) -> Self {
        Self { config, ..Default::default() }
    }

    /// Applies recording limits without resetting usage or an existing failure.
    ///
    /// Configure this before execution. Exceeding the budget stops recording, not execution;
    /// trace accessors and builders then return [`TraceError::LimitExceeded`].
    ///
    /// ```
    /// use revm_inspectors::tracing::{TraceLimits, TracingInspector, TracingInspectorConfig};
    /// let inspector = TracingInspector::new(TracingInspectorConfig::default_parity())
    ///     .with_limits(TraceLimits::default().set_max_recorded_bytes(Some(32 * 1024 * 1024)));
    /// // Execute with the inspector, then propagate failure before building a response:
    /// let builder = inspector.into_geth_builder()?;
    /// # Ok::<(), revm_inspectors::tracing::TraceError>(())
    /// ```
    pub fn with_limits(mut self, limits: TraceLimits) -> Self {
        self.budget.limits = limits;
        self.budget.record(0);
        self
    }

    /// Returns the recording limits, preserved by [`Self::fuse`].
    pub const fn limits(&self) -> TraceLimits {
        self.budget.limits
    }

    /// Returns the cumulative recording charge since the last reset.
    pub const fn recorded_bytes(&self) -> usize {
        self.budget.recorded
    }

    /// Reports whether recording failed. The first error persists until [`Self::fuse`].
    pub fn check_limits(&self) -> Result<(), TraceError> {
        self.budget.check()
    }

    /// Resets the inspector to its initial state of [Self::new].
    /// This makes the inspector ready to be used again.
    ///
    /// Note that this method has no effect on the allocated capacity of the vector.
    #[inline]
    pub fn fuse(&mut self) {
        let Self {
            traces,
            trace_stack,
            log_count,
            last_journal_len,
            spec_id,
            record_step_end,
            recorded_steps,
            budget,
            // kept
            config,
            reusable_step_vecs,
        } = self;

        // if we record steps we can reuse the individual calltracestep vecs
        if config.record_steps {
            for node in &mut traces.arena {
                // move out and store the reusable steps vec
                let mut steps = mem::take(&mut node.trace.steps);
                // ensure steps are cleared
                steps.clear();
                reusable_step_vecs.push(steps);
            }
        }

        traces.clear();
        trace_stack.clear();
        spec_id.take();
        *log_count = 0;
        *last_journal_len = 0;
        *record_step_end = false;
        *recorded_steps = 0;
        budget.reset();
    }

    /// Resets the inspector to it's initial state of [Self::new].
    #[inline]
    pub fn fused(mut self) -> Self {
        self.fuse();
        self
    }

    /// Returns the config of the inspector.
    pub const fn config(&self) -> &TracingInspectorConfig {
        &self.config
    }

    /// Returns a mutable reference to the config of the inspector.
    pub fn config_mut(&mut self) -> &mut TracingInspectorConfig {
        &mut self.config
    }

    /// Updates the config of the inspector.
    pub fn update_config(
        &mut self,
        f: impl FnOnce(TracingInspectorConfig) -> TracingInspectorConfig,
    ) {
        self.config = f(self.config);
    }

    /// Gets the recorded call traces, or the recording limit error.
    pub fn traces(&self) -> Result<&CallTraceArena, TraceError> {
        self.check_limits()?;
        Ok(&self.traces)
    }

    #[doc(hidden)]
    #[deprecated = "use `traces` instead"]
    pub fn get_traces(&self) -> Result<&CallTraceArena, TraceError> {
        self.traces()
    }

    /// Gets mutable recorded traces, or the recording limit error.
    ///
    /// Changes made by the caller are not included in the recording budget.
    pub fn traces_mut(&mut self) -> Result<&mut CallTraceArena, TraceError> {
        self.check_limits()?;
        Ok(&mut self.traces)
    }

    #[doc(hidden)]
    #[deprecated = "use `traces_mut` instead"]
    pub fn get_traces_mut(&mut self) -> Result<&mut CallTraceArena, TraceError> {
        self.traces_mut()
    }

    /// Consumes the inspector and returns recorded call traces, or the recording limit error.
    pub fn into_traces(self) -> Result<CallTraceArena, TraceError> {
        self.check_limits()?;
        Ok(self.traces)
    }

    /// Manually set the gas used of the root trace.
    ///
    /// This is useful if the root trace's gasUsed should mirror the actual gas used by the
    /// transaction.
    ///
    /// This allows setting it manually by consuming the execution result's gas for example.
    #[inline]
    pub fn set_transaction_gas_used(&mut self, gas_used: u64) {
        if let Some(node) = self.traces.arena.first_mut() {
            node.trace.gas_used = gas_used;
        }
    }

    /// Manually set the gas limit of the debug root trace.
    ///
    /// This is useful if the debug root trace's gasUsed should mirror the actual gas used by the
    /// transaction.
    ///
    /// This allows setting it manually by consuming the execution result's gas for example.
    #[inline]
    pub fn set_transaction_gas_limit(&mut self, gas_limit: u64) {
        if let Some(node) = self.traces.arena.first_mut() {
            node.trace.gas_limit = gas_limit;
        }
    }

    /// Convenience function for [ParityTraceBuilder::set_transaction_gas_used] that consumes the
    /// type.
    #[inline]
    pub fn with_transaction_gas_used(mut self, gas_used: u64) -> Self {
        self.set_transaction_gas_used(gas_used);
        self
    }

    /// Work with [TracingInspector::set_transaction_gas_limit] function
    #[inline]
    pub fn with_transaction_gas_limit(mut self, gas_limit: u64) -> Self {
        self.set_transaction_gas_limit(gas_limit);
        self
    }

    /// Manually set the caller address of the root trace.
    ///
    /// This is useful for custom transaction types (e.g. account abstraction batches) where the
    /// EVM's call entry point may not reflect the actual transaction sender.
    #[inline]
    pub fn set_transaction_caller(&mut self, caller: Address) {
        if let Some(node) = self.traces.arena.first_mut() {
            node.trace.caller = caller;
        }
    }

    /// Consumes the inspector and returns a [ParityTraceBuilder], or the recording limit error.
    #[inline]
    pub fn into_parity_builder(self) -> Result<ParityTraceBuilder, TraceError> {
        self.check_limits()?;
        Ok(ParityTraceBuilder::new(self.traces.arena, self.spec_id, self.config))
    }

    /// Consumes the inspector and returns a [GethTraceBuilder], or the recording limit error.
    #[inline]
    pub fn into_geth_builder(self) -> Result<GethTraceBuilder<'static>, TraceError> {
        self.check_limits()?;
        let builder = GethTraceBuilder::new(self.traces.arena);
        Ok(match self.spec_id {
            Some(spec_id) => builder.with_spec_id(spec_id),
            None => builder,
        })
    }

    /// Returns a [GethTraceBuilder] without consuming the inspector, or the recording limit error.
    ///
    /// This can be useful for multiple transaction tracing (block) where this inspector can be
    /// reused for each transaction but caller must ensure that the traces are cleared before
    /// starting a new transaction: [`Self::fuse`]
    #[inline]
    pub fn geth_builder(&self) -> Result<GethTraceBuilder<'_>, TraceError> {
        self.check_limits()?;
        let builder = GethTraceBuilder::new_borrowed(&self.traces.arena);
        Ok(match self.spec_id {
            Some(spec_id) => builder.with_spec_id(spec_id),
            None => builder,
        })
    }

    /// Returns true if we're no longer in the context of the root call.
    fn is_deep(&self) -> bool {
        // the root call will always be the first entry in the trace stack
        !self.trace_stack.is_empty()
    }

    /// Returns true if this a call to a precompile contract.
    ///
    /// Returns true if the `to` address is a precompile contract and the value is zero.
    #[inline]
    fn is_precompile_call<CTX: ContextTr<Journal: JournalExt>>(
        &self,
        context: &CTX,
        to: &Address,
        value: &U256,
    ) -> bool {
        if context.journal_ref().precompile_addresses().contains(to) {
            // only if this is _not_ the root call
            return self.is_deep() && value.is_zero();
        }
        false
    }

    /// Returns the currently active call trace.
    ///
    /// This will be the last call trace pushed to the stack: the call we entered most recently.
    #[track_caller]
    #[inline]
    fn active_trace(&self) -> Option<&CallTraceNode> {
        self.trace_stack.last().map(|idx| &self.traces.arena[*idx])
    }

    /// Returns the last trace [CallTrace] index from the stack.
    ///
    /// This will be the currently active call trace.
    ///
    /// # Panics
    ///
    /// If no [CallTrace] was pushed
    #[track_caller]
    #[inline]
    fn last_trace_idx(&self) -> usize {
        self.trace_stack.last().copied().expect("can't start step without starting a trace first")
    }

    /// Returns a mutable reference to the last trace [CallTrace] from the stack.
    #[track_caller]
    fn last_trace(&mut self) -> &mut CallTraceNode {
        let idx = self.last_trace_idx();
        &mut self.traces.arena[idx]
    }

    /// _Removes_ the last trace [CallTrace] index from the stack.
    ///
    /// # Panics
    ///
    /// If no [CallTrace] was pushed
    #[track_caller]
    #[inline]
    fn pop_trace_idx(&mut self) -> usize {
        self.trace_stack.pop().expect("more traces were filled than started")
    }

    /// Charge frame metadata, the active frame index, parent links, and input before retention.
    fn record_frame(&mut self, input_len: usize) -> bool {
        let links = if self.trace_stack.is_empty() {
            0
        } else {
            mem::size_of::<usize>() + mem::size_of::<TraceMemberOrder>()
        };
        self.budget.record(
            mem::size_of::<CallTraceNode>()
                .saturating_add(mem::size_of::<usize>())
                .saturating_add(links)
                .saturating_add(input_len),
        )
    }

    /// Starts tracking a new trace.
    ///
    /// Invoked on [Inspector::call].
    #[allow(clippy::too_many_arguments)]
    fn start_trace_on_call<CTX: ContextTr>(
        &mut self,
        context: &mut CTX,
        address: Address,
        input_data: Bytes,
        value: U256,
        kind: CallKind,
        caller: Address,
        gas_limit: u64,
        maybe_precompile: Option<bool>,
    ) {
        // This will only be true if the inspector is configured to exclude precompiles and the call
        // is to a precompile
        let push_kind = if maybe_precompile.unwrap_or(false) {
            // We don't want to track precompiles
            PushTraceKind::PushOnly
        } else {
            PushTraceKind::PushAndAttachToParent
        };

        // find an empty steps vec or create a new one
        let steps = self.reusable_step_vecs.pop().unwrap_or_default();

        // the currently active call is the parent of the new call
        let parent = self.trace_stack.last().copied().unwrap_or_default();

        self.trace_stack.push(self.traces.push_trace(
            parent,
            push_kind,
            CallTrace {
                depth: context.journal().depth(),
                address,
                kind,
                data: input_data,
                value,
                status: None,
                caller,
                maybe_precompile,
                gas_limit,
                steps,
                ..Default::default()
            },
        ));
    }

    /// Fills the current trace with the outcome of a call.
    ///
    /// Invoked on [Inspector::call_end].
    ///
    /// # Panics
    ///
    /// This expects an existing trace [Self::start_trace_on_call]
    fn fill_trace_on_call_end(
        &mut self,
        result: &InterpreterResult,
        created_address: Option<Address>,
    ) {
        if !self.budget.record(result.output.len()) {
            return;
        }
        let InterpreterResult { result, ref output, ref gas } = *result;

        let trace_idx = self.pop_trace_idx();
        let trace = &mut self.traces.arena[trace_idx].trace;

        trace.gas_used = gas.total_gas_spent();
        trace.gas_refund_counter = gas.refunded().max(0) as u64;

        trace.status = Some(result);
        trace.success = trace.status.is_some_and(|status| status.is_ok());
        trace.output = output.clone();

        if let Some(address) = created_address {
            // A new contract was created via CREATE
            trace.address = address;
        }
    }

    /// Starts tracking a step
    ///
    /// Invoked on [Inspector::step]
    ///
    /// # Panics
    ///
    /// This expects an existing [CallTrace], in other words, this panics if not within the context
    /// of a call.
    #[cold]
    fn start_step<CTX: ContextTr<Journal: JournalExt>>(
        &mut self,
        interp: &mut Interpreter,
        context: &mut CTX,
    ) {
        // We always want an OpCode, even it is unknown because it could be an additional opcode
        // that not a known constant.
        let op = OpCode::new_or_unknown(interp.bytecode.opcode());

        if self.config.record_step_deltas {
            self.finish_call_step(interp);
            if self.budget.error.is_some() {
                return;
            }
        }

        let record = self.config.should_record_opcode(op)
            && self.config.step_limit.is_none_or(|limit| self.recorded_steps < limit.get());
        self.record_step_end = record;
        if !record {
            return;
        }

        let stack_len = if self.config.record_stack_snapshots.is_all()
            || self.config.record_stack_snapshots.is_full()
        {
            interp.stack.len()
        } else if self.config.record_stack_snapshots.is_top() {
            interp.stack.len().min(1)
        } else {
            0
        };
        let memory_len = if self.config.record_memory_snapshots {
            interp.memory.borrow().context_memory().len()
        } else {
            0
        };
        let return_len = if self.config.record_returndata_snapshots {
            interp.return_data.buffer().len()
        } else {
            0
        };
        let immediate_len = if self.config.record_immediate_bytes {
            immediate_size(&interp.bytecode) as usize
        } else {
            0
        };
        let bytes = mem::size_of::<CallTraceStep>()
            .saturating_add(mem::size_of::<TraceMemberOrder>())
            .saturating_add(stack_len.saturating_mul(mem::size_of::<U256>()))
            .saturating_add(memory_len)
            .saturating_add(return_len)
            .saturating_add(immediate_len);
        if !self.budget.record(bytes) {
            return;
        }
        self.recorded_steps += 1;
        let trace_idx = self.last_trace_idx();
        let node = &mut self.traces.arena[trace_idx];

        // Reuse the memory from the previous step if:
        // - there is not opcode filter -- in this case we cannot rely on the order of steps
        // - it exists and has not modified memory
        let memory = self.config.record_memory_snapshots.then(|| {
            if self.config.record_opcodes_filter.is_none() {
                if let Some(prev) = node.trace.steps.last() {
                    if !prev.op.modifies_memory() {
                        if let Some(memory) = &prev.memory {
                            return memory.clone();
                        }
                    }
                }
            }
            RecordedMemory::new(&interp.memory.borrow().context_memory())
        });

        let stack = if self.config.record_stack_snapshots.is_all()
            || self.config.record_stack_snapshots.is_full()
        {
            Some(interp.stack.data().as_slice().into())
        } else if self.config.record_stack_snapshots.is_top() {
            let top = interp.stack.data().last().map(core::slice::from_ref).unwrap_or_default();
            Some(top.into())
        } else {
            None
        };
        let returndata = if self.config.record_returndata_snapshots {
            interp.return_data.buffer().clone()
        } else {
            Bytes::new()
        };

        let gas_used = gas_used(
            interp.runtime_flag.spec_id(),
            interp.gas.total_gas_spent(),
            interp.gas.refunded() as u64,
        );

        let mut immediate_bytes = None;
        if self.config.record_immediate_bytes {
            let size = immediate_size(&interp.bytecode);
            if size != 0 {
                immediate_bytes = Some(Bytes::copy_from_slice(
                    &interp.bytecode.read_slice(size as usize + 1)[1..],
                ));
            }
        }

        self.last_journal_len = context.journal_ref().journal().len();

        let step_idx = node.trace.steps.len();
        node.trace.steps.push(CallTraceStep {
            pc: interp.bytecode.pc(),
            op,
            stack,
            memory,
            returndata,
            gas_remaining: interp.gas.remaining(),
            gas_refund_counter: interp.gas.refunded() as u64,
            gas_used,
            immediate_bytes,
            state_gas_cost: None,
            state_gas_reservoir: SpecId::is_enabled_in(
                interp.runtime_flag.spec_id(),
                SpecId::AMSTERDAM,
            )
            .then_some(interp.gas.reservoir()),
            state_gas_spent: interp.gas.state_gas_spent(),

            // These fields will be populated in `step_end`.
            push_stack: None,
            gas_cost: 0,
            storage_change: None,
            status: None,

            // This is never populated in `TracingInspector`.
            decoded: None,
        });

        node.ordering.push(TraceMemberOrder::Step(step_idx));

        if self.config.record_step_deltas {
            let write_range = memory_write_range(op.get(), interp.stack.data());
            if write_range.is_some() || node.trace.steps[step_idx].is_call_like_op() {
                if !self.budget.record(mem::size_of::<StepDelta>()) {
                    return;
                }
                node.trace.step_deltas.push(StepDelta {
                    step: step_idx,
                    write_range,
                    ..Default::default()
                });
            }
        }
    }

    /// Completes the delta of the last step if it is a call-like step whose frame returned.
    ///
    /// The result of a CALL or CREATE is pushed to the stack, the returned data copied into
    /// memory and the unused gas returned only after `step_end`, once the parent interpreter
    /// resumes, so this runs at the start of its next step.
    fn finish_call_step(&mut self, interp: &Interpreter) {
        let trace_idx = self.last_trace_idx();
        let trace = &mut self.traces.arena[trace_idx].trace;
        let step_idx = trace.steps.len().wrapping_sub(1);
        let Some((step, delta)) = trace.steps.last_mut().zip(trace.step_deltas.last_mut()) else {
            return;
        };
        if delta.step != step_idx
            || !step.is_call_like_op()
            || step.is_error()
            || delta.gas_remaining_after.is_some()
        {
            return;
        }

        delta.gas_remaining_after = Some(interp.gas.remaining());
        if step.push_stack.is_some() {
            if !self.budget.record(interp.stack.len().min(1) * mem::size_of::<U256>()) {
                return;
            }
            step.push_stack = Some(interp.stack.data().last().copied().into_iter().collect());
        }
        // CALL only overwrites the returned bytes; the rest of its output buffer is unchanged.
        if let Some(range) = &mut delta.write_range {
            range.end = range.start + range.len().min(interp.return_data.buffer().len());
        }
        delta.record_memory_write(&interp.memory.borrow().context_memory(), &mut self.budget);
    }

    /// Fills the current trace with the output of a step.
    ///
    /// Invoked on [Inspector::step_end].
    #[cold]
    fn fill_step_on_step_end<CTX: ContextTr<Journal: JournalExt>>(
        &mut self,
        interp: &mut Interpreter,
        context: &mut CTX,
    ) {
        // No need to reset here, since it is only read here and it will be overwritten by the next
        // step.
        if !self.record_step_end {
            return;
        }

        let trace_idx = self.last_trace_idx();
        let node = &mut self.traces.arena[trace_idx];
        let step_idx = node.trace.steps.len() - 1;
        let step = &mut node.trace.steps[step_idx];

        // See comments in `start_step`.
        debug_assert!(
            step.push_stack.is_none()
                && step.gas_cost == 0
                && step.storage_change.is_none()
                && step.status.is_none()
                && step.decoded.is_none(),
            "step in step_end is already filled: {trace_idx} -> {step:#?}",
        );

        if self.config.record_stack_snapshots.is_all()
            || self.config.record_stack_snapshots.is_pushes()
        {
            let outputs = if step.op.is_valid() { step.op.outputs() as usize } else { 0 };
            if !self.budget.record(outputs.min(interp.stack.len()) * mem::size_of::<U256>()) {
                return;
            }
            step.push_stack = Some(
                interp
                    .stack
                    .data()
                    .get(interp.stack.len().saturating_sub(outputs)..)
                    .unwrap_or_default()
                    .into(),
            );
        }

        let journal = context.journal_ref().journal();

        // If journal has not changed, there is no state change to be recorded.
        if self.config.record_state_diff && journal.len() != self.last_journal_len {
            let op = step.op.get();

            step.storage_change = if matches!(op, opcode::SLOAD | opcode::SSTORE) {
                let reason = match op {
                    opcode::SLOAD => StorageChangeReason::SLOAD,
                    opcode::SSTORE => StorageChangeReason::SSTORE,
                    _ => unreachable!(),
                };

                match journal.last() {
                    Some(JournalEntry::StorageChanged { address, key, had_value }) => {
                        // SAFETY: (Address,key) exists if part if StorageChange
                        let value =
                            context.journal_ref().evm_state()[address].storage[key].present_value();
                        let change =
                            StorageChange { key: *key, value, had_value: Some(*had_value), reason };
                        if !self.budget.record(mem::size_of::<StorageChange>()) {
                            return;
                        }
                        Some(Box::new(change))
                    }
                    Some(JournalEntry::StorageWarmed { key, address }) => {
                        // SAFETY: (Address,key) exists if part if StorageChange
                        let value =
                            context.journal_ref().evm_state()[address].storage[key].present_value();
                        let change = StorageChange { key: *key, value, had_value: None, reason };
                        if !self.budget.record(mem::size_of::<StorageChange>()) {
                            return;
                        }
                        Some(Box::new(change))
                    }
                    _ => None,
                }
            } else {
                None
            };
        }

        // The gas cost is the difference between the recorded gas remaining at the start of the
        // step the remaining gas here, at the end of the step.
        // TODO: Figure out why this can overflow. https://github.com/paradigmxyz/revm-inspectors/pull/38
        step.gas_cost = step.gas_remaining.saturating_sub(interp.gas.remaining());
        let state_gas_delta = interp.gas.state_gas_spent().saturating_sub(step.state_gas_spent);
        if step.state_gas_reservoir.is_some() && state_gas_delta != 0 {
            step.state_gas_cost = Some(state_gas_delta);
        }

        // set the status
        step.status = interp.bytecode.action().as_ref().and_then(|i| i.instruction_result());

        // Call-like steps write memory only once the parent frame resumes, see
        // `finish_call_step`.
        if self.config.record_step_deltas && !step.is_error() && !step.is_call_like_op() {
            // Gas credits, such as EIP-8037 storage restoration, cannot be recovered from the
            // saturated unsigned gas cost.
            let gas_remaining_after =
                (interp.gas.remaining() > step.gas_remaining).then_some(interp.gas.remaining());
            if gas_remaining_after.is_some()
                && node.trace.step_deltas.last().is_none_or(|delta| delta.step != step_idx)
            {
                if !self.budget.record(mem::size_of::<StepDelta>()) {
                    return;
                }
                node.trace.step_deltas.push(StepDelta { step: step_idx, ..Default::default() });
            }
            if let Some(delta) =
                node.trace.step_deltas.last_mut().filter(|delta| delta.step == step_idx)
            {
                delta.gas_remaining_after = gas_remaining_after;
                delta.record_memory_write(
                    &interp.memory.borrow().context_memory(),
                    &mut self.budget,
                );
            }
        }
    }
}

impl<CTX> Inspector<CTX> for TracingInspector
where
    CTX: ContextTr<Journal: JournalExt>,
{
    fn initialize_interp(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        if self.budget.error.is_some() {
            return;
        }
        if self.spec_id.is_none() {
            self.spec_id = Some(interp.runtime_flag.spec_id());
        }
        if self.config.record_bytecode {
            if !self.budget.record(interp.bytecode.original_byte_slice().len()) {
                return;
            }
            self.last_trace().trace.bytecode = Some(interp.bytecode.original_bytes());
        }
    }

    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if self.budget.error.is_some() {
            return;
        }
        if self.config.record_steps {
            self.start_step(interp, context);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if self.budget.error.is_some() {
            return;
        }
        if self.config.record_steps {
            self.fill_step_on_step_end(interp, context);
        }
    }

    fn log(&mut self, _context: &mut CTX, log: Log) {
        if self.budget.error.is_some() {
            return;
        }
        if self.config.record_logs {
            let bytes = mem::size_of::<CallLog>() + mem::size_of::<TraceMemberOrder>();
            if !self.budget.record(
                bytes
                    .saturating_add(log.data.data.len())
                    .saturating_add(mem::size_of_val(log.data.topics())),
            ) {
                return;
            }
            // index starts at 0
            let log_count = self.log_count;
            self.log_count += 1;
            let trace = self.last_trace();
            trace.ordering.push(TraceMemberOrder::Log(trace.logs.len()));
            trace.logs.push(
                CallLog::from(log)
                    .with_position(trace.children.len() as u64)
                    .with_index(log_count as u64),
            );
        }
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        if self.budget.error.is_some() {
            return None;
        }
        if self.spec_id.is_none() {
            self.spec_id = Some(context.cfg().spec().into());
        }

        // determine correct `from` and `to` based on the call scheme
        let (from, to) = match inputs.scheme {
            CallScheme::DelegateCall | CallScheme::CallCode => {
                (inputs.target_address, inputs.bytecode_address)
            }
            _ => (inputs.caller, inputs.target_address),
        };

        let value = if matches!(inputs.scheme, CallScheme::DelegateCall) {
            // for delegate calls we need to use the value of the top trace
            if let Some(parent) = self.active_trace() {
                parent.trace.value
            } else {
                inputs.call_value()
            }
        } else {
            inputs.call_value()
        };

        // if calls to precompiles should be excluded, check whether this is a call to a precompile
        let maybe_precompile = self
            .config
            .exclude_precompile_calls
            .then(|| self.is_precompile_call(context, &to, &value));

        let input_len = if self.config.record_inputs { inputs.input.len() } else { 0 };
        if !self.record_frame(input_len) {
            return None;
        }
        let input =
            if self.config.record_inputs { inputs.input_data(context) } else { Bytes::new() };
        self.start_trace_on_call(
            context,
            to,
            input,
            value,
            inputs.scheme.into(),
            from,
            inputs.gas_limit,
            maybe_precompile,
        );

        None
    }

    fn call_end(&mut self, _: &mut CTX, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        self.fill_trace_on_call_end(&outcome.result, None);
    }

    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        if self.budget.error.is_some() {
            return None;
        }
        if self.spec_id.is_none() {
            self.spec_id = Some(context.cfg().spec().into());
        }

        let nonce = context.journal_mut().load_account(inputs.caller()).ok()?.info.nonce;
        let input_len = if self.config.record_inputs { inputs.init_code().len() } else { 0 };
        if !self.record_frame(input_len) {
            return None;
        }
        self.start_trace_on_call(
            context,
            inputs.created_address(nonce),
            if self.config.record_inputs { inputs.init_code().clone() } else { Bytes::new() },
            inputs.value(),
            inputs.scheme().into(),
            inputs.caller(),
            inputs.gas_limit(),
            Some(false),
        );
        None
    }

    fn create_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        self.fill_trace_on_call_end(&outcome.result, outcome.address);
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        if self.budget.error.is_some() {
            return;
        }
        let node = self.last_trace();
        node.trace.selfdestruct_address = Some(contract);
        node.trace.selfdestruct_refund_target = Some(target);
        node.trace.selfdestruct_transferred_value = Some(value);
    }
}

/// Contains some contextual infos for a transaction execution that is made available to the JS
/// object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransactionContext {
    /// Hash of the block the tx is contained within.
    ///
    /// `None` if this is a call.
    pub block_hash: Option<B256>,
    /// Index of the transaction within a block.
    ///
    /// `None` if this is a call.
    pub tx_index: Option<usize>,
    /// Hash of the transaction being traced.
    ///
    /// `None` if this is a call.
    pub tx_hash: Option<B256>,
}

impl TransactionContext {
    /// Sets the block hash.
    pub const fn with_block_hash(mut self, block_hash: B256) -> Self {
        self.block_hash = Some(block_hash);
        self
    }

    /// Sets the index of the transaction within a block.
    pub const fn with_tx_index(mut self, tx_index: usize) -> Self {
        self.tx_index = Some(tx_index);
        self
    }

    /// Sets the hash of the transaction.
    pub const fn with_tx_hash(mut self, tx_hash: B256) -> Self {
        self.tx_hash = Some(tx_hash);
        self
    }
}

impl From<alloy_rpc_types_eth::TransactionInfo> for TransactionContext {
    fn from(tx_info: alloy_rpc_types_eth::TransactionInfo) -> Self {
        Self {
            block_hash: tx_info.block_hash,
            tx_index: tx_info.index.map(|idx| idx as usize),
            tx_hash: tx_info.hash,
        }
    }
}

/// A helper extension trait that _clones_ the input data from the shared mem buffer
pub(crate) trait CallInputExt {
    fn input_data<CTX: ContextTr>(&self, ctx: &mut CTX) -> Bytes;
}

impl CallInputExt for CallInputs {
    fn input_data<CTX: ContextTr>(&self, ctx: &mut CTX) -> Bytes {
        match &self.input {
            CallInput::SharedBuffer(range) => ctx
                .local()
                .shared_memory_buffer_slice(range.clone())
                .map(|slice| Bytes::copy_from_slice(&slice))
                .unwrap_or_default(),
            CallInput::Bytes(bytes) => bytes.clone(),
        }
    }
}

/// Returns the memory range the opcode writes, derived from its inputs on the stack.
///
/// Only writes are tracked: instructions that merely expand memory, like `MLOAD`, yield `None`.
fn memory_write_range(op: u8, stack: &[U256]) -> Option<Range<usize>> {
    let back = |index: usize| {
        stack.get(stack.len().checked_sub(index + 1)?).and_then(|v| usize::try_from(*v).ok())
    };
    let (offset, size) = match op {
        opcode::MSTORE => (back(0)?, 32),
        opcode::MSTORE8 => (back(0)?, 1),
        opcode::CALLDATACOPY | opcode::CODECOPY | opcode::RETURNDATACOPY | opcode::MCOPY => {
            (back(0)?, back(2)?)
        }
        opcode::EXTCODECOPY => (back(1)?, back(3)?),
        opcode::CALL | opcode::CALLCODE => (back(5)?, back(6)?),
        opcode::DELEGATECALL | opcode::STATICCALL => (back(4)?, back(5)?),
        _ => return None,
    };
    (size != 0).then_some(offset..offset.checked_add(size)?)
}
