use crate::{
    opcode::immediate_size,
    tracing::{
        arena::PushTraceKind,
        types::{
            CallKind, CallTraceNode, RecordedMemory, StorageChange, StorageChangeReason,
            TraceMemberOrder,
        },
        utils::gas_used,
    },
};
use alloy_primitives::{Address, Bytes, Log, B256, U256};
use revm::{
    interpreter::{
        opcode::{self},
        CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome, EOFCreateInputs,
        InstructionResult, Interpreter, InterpreterResult, OpCode,
    },
    primitives::SpecId,
    Database, EvmContext, Inspector, JournalEntry,
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

mod fourbyte;
pub use fourbyte::FourByteInspector;

mod opcount;
pub use opcount::OpcodeCountInspector;

pub mod types;
use types::{CallLog, CallTrace, CallTraceStep};

mod utils;

mod writer;
pub use writer::{TraceWriter, TraceWriterConfig};

#[cfg(feature = "js-tracer")]
pub mod js;

mod mux;
pub use mux::{Error as MuxError, MuxInspector};

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
    /// Records all call traces
    traces: CallTraceArena,
    /// Tracks active calls
    trace_stack: Vec<usize>,
    /// Tracks active steps
    step_stack: Vec<StackStep>,
    /// Tracks the return value of the last call
    last_call_return_data: Option<Bytes>,
    /// The spec id of the EVM.
    ///
    /// This is filled during execution.
    spec_id: Option<SpecId>,
}

// === impl TracingInspector ===

impl TracingInspector {
    /// Returns a new instance for the given config
    pub fn new(config: TracingInspectorConfig) -> Self {
        Self { config, ..Default::default() }
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
            step_stack,
            last_call_return_data,
            spec_id,
            // kept
            config: _,
        } = self;
        traces.clear();
        trace_stack.clear();
        step_stack.clear();
        last_call_return_data.take();
        spec_id.take();
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

    /// Gets a reference to the recorded call traces.
    pub const fn traces(&self) -> &CallTraceArena {
        &self.traces
    }

    #[doc(hidden)]
    #[deprecated = "use `traces` instead"]
    pub const fn get_traces(&self) -> &CallTraceArena {
        &self.traces
    }

    /// Gets a mutable reference to the recorded call traces.
    pub fn traces_mut(&mut self) -> &mut CallTraceArena {
        &mut self.traces
    }

    #[doc(hidden)]
    #[deprecated = "use `traces_mut` instead"]
    pub fn get_traces_mut(&mut self) -> &mut CallTraceArena {
        &mut self.traces
    }

    /// Consumes the inspector and returns the recorded call traces.
    pub fn into_traces(self) -> CallTraceArena {
        self.traces
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

    /// Consumes the Inspector and returns a [ParityTraceBuilder].
    #[inline]
    pub fn into_parity_builder(self) -> ParityTraceBuilder {
        ParityTraceBuilder::new(self.traces.arena, self.spec_id, self.config)
    }

    /// Consumes the Inspector and returns a [GethTraceBuilder].
    #[inline]
    pub fn into_geth_builder(self) -> GethTraceBuilder<'static> {
        GethTraceBuilder::new(self.traces.arena, self.config)
    }

    /// Returns the  [GethTraceBuilder] for the recorded traces without consuming the type.
    ///
    /// This can be useful for multiple transaction tracing (block) where this inspector can be
    /// reused for each transaction but caller must ensure that the traces are cleared before
    /// starting a new transaction: [`Self::fuse`]
    #[inline]
    pub fn geth_builder(&self) -> GethTraceBuilder<'_> {
        GethTraceBuilder::new_borrowed(&self.traces.arena, self.config)
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
    fn is_precompile_call<DB: Database>(
        &self,
        context: &EvmContext<DB>,
        to: &Address,
        value: &U256,
    ) -> bool {
        if context.precompiles.contains(to) {
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

    /// Starts tracking a new trace.
    ///
    /// Invoked on [Inspector::call].
    #[allow(clippy::too_many_arguments)]
    fn start_trace_on_call<DB: Database>(
        &mut self,
        context: &EvmContext<DB>,
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

        self.trace_stack.push(self.traces.push_trace(
            0,
            push_kind,
            CallTrace {
                depth: context.journaled_state.depth() as usize,
                address,
                kind,
                data: input_data,
                value,
                status: InstructionResult::Continue,
                caller,
                maybe_precompile,
                gas_limit,
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
    fn fill_trace_on_call_end<DB: Database>(
        &mut self,
        _context: &mut EvmContext<DB>,
        result: &InterpreterResult,
        created_address: Option<Address>,
    ) {
        let InterpreterResult { result, ref output, ref gas } = *result;

        let trace_idx = self.pop_trace_idx();
        let trace = &mut self.traces.arena[trace_idx].trace;

        trace.gas_used = gas.spent();

        trace.status = result;
        trace.success = trace.status.is_ok();
        trace.output = output.clone();

        self.last_call_return_data = Some(output.clone());

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
    fn start_step<DB: Database>(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        let trace_idx = self.last_trace_idx();
        let trace = &mut self.traces.arena[trace_idx];

        let step_idx = trace.trace.steps.len();
        // We always want an OpCode, even it is unknown because it could be an additional opcode
        // that not a known constant.
        let op = unsafe { OpCode::new_unchecked(interp.current_opcode()) };

        let record = self.config.should_record_opcode(op);

        self.step_stack.push(StackStep { trace_idx, step_idx, record });

        if !record {
            return;
        }

        // Reuse the memory from the previous step if:
        // - there is not opcode filter -- in this case we cannot rely on the order of steps
        // - it exists and has not modified memory
        let memory = self.config.record_memory_snapshots.then(|| {
            if self.config.record_opcodes_filter.is_none() {
                if let Some(prev) = trace.trace.steps.last() {
                    if !prev.op.modifies_memory() {
                        if let Some(memory) = &prev.memory {
                            return memory.clone();
                        }
                    }
                }
            }
            RecordedMemory::new(interp.shared_memory.context_memory())
        });

        let stack = if self.config.record_stack_snapshots.is_full() {
            Some(interp.stack.data().clone())
        } else {
            None
        };
        let returndata = self
            .config
            .record_returndata_snapshots
            .then(|| interp.return_data_buffer.clone())
            .unwrap_or_default();

        let gas_used =
            gas_used(context.spec_id(), interp.gas.spent(), interp.gas.refunded() as u64);

        let immediate_bytes = if self.config.record_immediate_bytes {
            let pc = interp.program_counter();
            let size = immediate_size(op, &interp.bytecode[pc + 1..]);
            let start = pc + 1;
            let end = pc + 1 + size as usize;
            (size > 0 && end <= interp.bytecode.len()).then(|| interp.bytecode.slice(start..end))
        } else {
            None
        };

        trace.trace.steps.push(CallTraceStep {
            depth: context.journaled_state.depth(),
            pc: interp.program_counter(),
            code_section_idx: interp.function_stack.current_code_idx,
            op,
            contract: interp.contract.target_address,
            stack,
            push_stack: None,
            memory,
            returndata,
            gas_remaining: interp.gas.remaining(),
            gas_refund_counter: interp.gas.refunded() as u64,
            gas_used,
            decoded: None,
            immediate_bytes,

            // fields will be populated end of call
            gas_cost: 0,
            storage_change: None,
            status: InstructionResult::Continue,
        });

        trace.ordering.push(TraceMemberOrder::Step(step_idx));
    }

    /// Fills the current trace with the output of a step.
    ///
    /// Invoked on [Inspector::step_end].
    #[cold]
    fn fill_step_on_step_end<DB: Database>(
        &mut self,
        interp: &mut Interpreter,
        context: &mut EvmContext<DB>,
    ) {
        let StackStep { trace_idx, step_idx, record } =
            self.step_stack.pop().expect("can't fill step without starting a step first");

        if !record {
            return;
        }

        let step = &mut self.traces.arena[trace_idx].trace.steps[step_idx];

        if self.config.record_stack_snapshots.is_pushes() {
            let start = interp.stack.len() - step.op.outputs() as usize;
            step.push_stack = Some(interp.stack.data()[start..].to_vec());
        }

        if self.config.record_state_diff {
            let op = step.op.get();

            let journal_entry = context
                .journaled_state
                .journal
                .last()
                // This should always work because revm initializes it as `vec![vec![]]`
                // See [JournaledState::new](revm::JournaledState)
                .expect("exists; initialized with vec")
                .last();

            step.storage_change = match (op, journal_entry) {
                (
                    opcode::SLOAD | opcode::SSTORE,
                    Some(JournalEntry::StorageChanged { address, key, had_value }),
                ) => {
                    // SAFETY: (Address,key) exists if part if StorageChange
                    let value = context.journaled_state.state[address].storage[key].present_value();
                    let reason = match op {
                        opcode::SLOAD => StorageChangeReason::SLOAD,
                        opcode::SSTORE => StorageChangeReason::SSTORE,
                        _ => unreachable!(),
                    };
                    let change =
                        StorageChange { key: *key, value, had_value: Some(*had_value), reason };
                    Some(change)
                }
                _ => None,
            };
        }

        // The gas cost is the difference between the recorded gas remaining at the start of the
        // step the remaining gas here, at the end of the step.
        // TODO: Figure out why this can overflow. https://github.com/paradigmxyz/revm-inspectors/pull/38
        step.gas_cost = step.gas_remaining.saturating_sub(interp.gas.remaining());

        // set the status
        step.status = interp.instruction_result;
    }
}

impl<DB> Inspector<DB> for TracingInspector
where
    DB: Database,
{
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        if self.config.record_steps {
            self.start_step(interp, context);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        if self.config.record_steps {
            self.fill_step_on_step_end(interp, context);
        }
    }

    fn log(&mut self, _interp: &mut Interpreter, _context: &mut EvmContext<DB>, log: &Log) {
        if self.config.record_logs {
            let trace = self.last_trace();
            trace.ordering.push(TraceMemberOrder::Log(trace.logs.len()));
            trace.logs.push(CallLog::from(log.clone()).with_position(trace.children.len() as u64));
        }
    }

    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        // determine correct `from` and `to` based on the call scheme
        let (from, to) = match inputs.scheme {
            CallScheme::DelegateCall | CallScheme::CallCode | CallScheme::ExtDelegateCall => {
                (inputs.target_address, inputs.bytecode_address)
            }
            _ => (inputs.caller, inputs.target_address),
        };

        let value =
            if matches!(inputs.scheme, CallScheme::DelegateCall | CallScheme::ExtDelegateCall) {
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

        self.start_trace_on_call(
            context,
            to,
            inputs.input.clone(),
            value,
            inputs.scheme.into(),
            from,
            inputs.gas_limit,
            maybe_precompile,
        );

        None
    }

    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        self.fill_trace_on_call_end(context, &outcome.result, None);
        outcome
    }

    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        let _ = context.load_account(inputs.caller);
        let nonce = context.journaled_state.account(inputs.caller).info.nonce;
        self.start_trace_on_call(
            context,
            inputs.created_address(nonce),
            inputs.init_code.clone(),
            inputs.value,
            inputs.scheme.into(),
            inputs.caller,
            inputs.gas_limit,
            Some(false),
        );

        None
    }

    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        self.fill_trace_on_call_end(context, &outcome.result, outcome.address);
        outcome
    }

    fn eofcreate(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        let address = if let Some(address) = inputs.kind.created_address() {
            *address
        } else {
            let _ = context.load_account(inputs.caller);
            let nonce = context.journaled_state.account(inputs.caller).info.nonce;
            inputs.caller.create(nonce)
        };
        self.start_trace_on_call(
            context,
            address,
            Bytes::new(),
            inputs.value,
            CallKind::EOFCreate,
            inputs.caller,
            inputs.gas_limit,
            Some(false),
        );

        None
    }

    fn eofcreate_end(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &EOFCreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        self.fill_trace_on_call_end(context, &outcome.result, outcome.address);
        outcome
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        let node = self.last_trace();
        node.trace.selfdestruct_address = Some(contract);
        node.trace.selfdestruct_refund_target = Some(target);
        node.trace.selfdestruct_transferred_value = Some(value);
    }
}

/// Struct keeping track of internal inspector steps stack.
#[derive(Clone, Copy, Debug)]
struct StackStep {
    /// Whether this step should be recorded.
    ///
    /// This is set to `false` if [OpcodeFilter] is configured and this step's opcode is not
    /// enabled for tracking
    record: bool,
    /// Idx of the trace node this step belongs.
    trace_idx: usize,
    /// Idx of this step in the [CallTrace::steps].
    ///
    /// Please note that if `record` is `false`, this will still contain a value, but the step will
    /// not appear in the steps list.
    step_idx: usize,
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
