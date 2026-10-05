//! Javascript inspector

use crate::tracing::{
    config::TraceStyle,
    js::{
        bindings::{
            CallFrame, Contract, FrameKind, FrameResult, JsEvmContext, OpcodeNames, PreStep,
            ReusableCallFrame, ReusableEvmDb, ReusableFrameResult, ReusableStepLog, StepInfo,
        },
        builtins::{register_builtins, to_serde_value, PrecompileList},
    },
    types::CallKind,
    utils, CallInputExt, TransactionContext,
};
use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use alloy_primitives::{Address, Bytes, U256};
use boa_engine::{js_string, Context, JsError, JsObject, JsResult, JsValue, Script, Source};
use core::sync::atomic::{AtomicBool, Ordering};
use revm::{
    bytecode::opcode,
    context::JournalTr,
    context_interface::{
        result::{ExecutionResult, HaltReasonTr, Output, ResultAndState},
        Block, ContextError, ContextTr, TransactTo, Transaction,
    },
    database::WrapDatabaseRef,
    inspector::JournalExt,
    interpreter::{
        interpreter_types::{Jumps, LoopControl},
        CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome, Gas, InstructionResult,
        Interpreter, InterpreterAction, InterpreterResult,
    },
    DatabaseRef, Inspector,
};

pub use boa_engine::vm::RuntimeLimits;

pub(crate) mod bindings;
pub(crate) mod builtins;

#[cfg(test)]
mod interrupt_tests;

/// The maximum number of iterations in a loop.
///
/// Once exceeded, the loop will throw an error.
// An empty loop with this limit takes around 50ms to fail.
pub const LOOP_ITERATION_LIMIT: u64 = 200_000;

/// The recursion limit for function calls.
///
/// Once exceeded, the function will throw an error.
pub const RECURSION_LIMIT: usize = 10_000;

/// A javascript inspector that will delegate inspector functions to javascript functions
///
/// See also <https://geth.ethereum.org/docs/developers/evm-tracing/custom-tracer#custom-javascript-tracing>
pub struct JsInspector {
    ctx: Context,
    /// The original javascript code used to create this inspector.
    code: String,
    /// The parsed tracer script, evaluated again by [`Self::fuse`] to get a fresh tracer object.
    script: Script,
    /// The input config object.
    config: serde_json::Value,
    /// The evaluated object that contains the inspector functions.
    obj: JsObject,
    /// [`Self::obj`] as the `this` value of the callbacks.
    this: JsValue,
    /// The context of the transaction that is being inspected.
    transaction_context: TransactionContext,

    /// The javascript function that will be called when the result is requested.
    result_fn: JsObject,
    fault_fn: JsObject,

    // EVM inspector hook functions
    /// Invoked when the EVM enters a new call that is _NOT_ the top level call.
    ///
    /// Corresponds to [Inspector::call] and [Inspector::create_end] but is also invoked on
    /// [Inspector::selfdestruct].
    enter_fn: Option<JsObject>,
    /// Invoked when the EVM exits a call that is _NOT_ the top level call.
    ///
    /// Corresponds to [Inspector::call_end] and [Inspector::create_end] but also invoked after
    /// selfdestruct.
    exit_fn: Option<JsObject>,
    /// Executed before each instruction is executed.
    step_fn: Option<JsObject>,
    /// Opcode name strings shared by all step wrappers this inspector creates.
    op_names: OpcodeNames,
    /// Reused step wrapper to avoid rebuilding the JS object graph per opcode.
    reusable_step_log: ReusableStepLog,
    /// Reused frame wrapper to avoid rebuilding the JS object graph per enter callback.
    reusable_call_frame: ReusableCallFrame,
    /// Reused frame wrapper to avoid rebuilding the JS object graph per exit callback.
    reusable_frame_result: ReusableFrameResult,
    /// Reused database wrapper shared by all callbacks.
    reusable_db: ReusableEvmDb,
    /// Keeps track of the current call stack.
    call_stack: Vec<CallStackItem>,
    /// Id assigned to the next call, see [`CallStackItem::id`]. Never reset, so ids stay unique
    /// across transactions traced by the same inspector.
    next_call_id: u64,
    /// Marker to track whether the precompiles have been registered.
    precompiles_registered: bool,
    /// Whether `step` recorded a step that `step_end` still has to report.
    step_pending: bool,
    /// Total gas spent before the pending step, to compute the step's cost in `step_end`.
    gas_spent_before: u64,
    /// Optional cancellation signal shared with the caller.
    interrupt: Option<JsInspectorInterrupt>,
}

impl core::fmt::Debug for JsInspector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JsInspector")
            .field("code", &self.code)
            .field("config", &self.config)
            .field("transaction_context", &self.transaction_context)
            .field("call_stack", &self.call_stack)
            .finish_non_exhaustive()
    }
}

impl JsInspector {
    /// Creates a new inspector from a javascript code snipped that evaluates to an object with the
    /// expected fields and a config object.
    ///
    /// The object must have the following fields:
    ///  - `result`: a function that will be called when the result is requested.
    ///  - `fault`: a function that will be called when the transaction fails.
    ///
    /// Optional functions are invoked during inspection:
    /// - `setup`: a function that will be called before the inspection starts.
    /// - `enter`: a function that will be called when the execution enters a new call.
    /// - `exit`: a function that will be called when the execution exits a call.
    /// - `step`: a function that will be called when the execution steps to the next instruction.
    ///
    /// This also accepts a sender half of a channel to communicate with the database service so the
    /// DB can be queried from inside the inspector.
    pub fn new(code: String, config: serde_json::Value) -> Result<Self, JsInspectorError> {
        Self::with_transaction_context(code, config, Default::default())
    }

    /// Creates a new inspector from a javascript code snippet. See also [Self::new].
    ///
    /// This also accepts a [TransactionContext] that gives the JS code access to some contextual
    /// transaction infos.
    pub fn with_transaction_context(
        code: String,
        config: serde_json::Value,
        transaction_context: TransactionContext,
    ) -> Result<Self, JsInspectorError> {
        // Instantiate the execution context
        let mut ctx = Context::default();

        // Apply the default runtime limits
        // This is a safe guard to prevent infinite loops
        ctx.runtime_limits_mut().set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
        ctx.runtime_limits_mut().set_recursion_limit(RECURSION_LIMIT);

        register_builtins(&mut ctx)?;

        // parse the code
        let wrapped = format!("({code})");
        let script = Script::parse(Source::from_bytes(wrapped.as_bytes()), None, &mut ctx)
            .map_err(JsInspectorError::EvalCode)?;

        let JsTracerObject { obj, result_fn, fault_fn, enter_fn, exit_fn, step_fn } =
            JsTracerObject::evaluate(&script, &config, &mut ctx)?;

        let op_names = OpcodeNames::new();
        let reusable_step_log =
            ReusableStepLog::new(&mut ctx, op_names.clone()).map_err(JsInspectorError::EvalCode)?;
        let reusable_call_frame =
            ReusableCallFrame::new(&mut ctx).map_err(JsInspectorError::EvalCode)?;
        let reusable_frame_result =
            ReusableFrameResult::new(&mut ctx).map_err(JsInspectorError::EvalCode)?;
        let reusable_db = ReusableEvmDb::new(&mut ctx).map_err(JsInspectorError::EvalCode)?;

        Ok(Self {
            ctx,
            code,
            script,
            config,
            this: obj.clone().into(),
            obj,
            transaction_context,
            result_fn,
            fault_fn,
            enter_fn,
            exit_fn,
            step_fn,
            op_names,
            reusable_step_log,
            reusable_call_frame,
            reusable_frame_result,
            reusable_db,
            call_stack: Default::default(),
            next_call_id: 1,
            precompiles_registered: false,
            step_pending: false,
            gas_spent_before: 0,
            interrupt: None,
        })
    }

    /// Returns the config object.
    pub const fn config(&self) -> &serde_json::Value {
        &self.config
    }

    /// Creates a fresh inspector from the same code and config, sharing the interrupt flag.
    pub fn try_clone(&self) -> Result<Self, JsInspectorError> {
        self.ensure_not_interrupted()?;
        let mut inspector = Self::new(self.code.clone(), self.config.clone())?;
        inspector.interrupt = self.interrupt.clone();
        Ok(inspector)
    }

    /// Resets the inspector to its initial state so it can be used for the next transaction.
    ///
    /// This evaluates the tracer script again in the existing JS context, which yields a fresh
    /// tracer object (and invokes its `setup` function) without parsing the script again. Global
    /// state the previous tracer object may have modified, e.g. prototypes, is kept. Callback
    /// wrappers are recreated so their own properties do not carry over between transactions.
    pub fn fuse(&mut self) -> Result<(), JsInspectorError> {
        self.ensure_not_interrupted()?;
        let JsTracerObject { obj, result_fn, fault_fn, enter_fn, exit_fn, step_fn } =
            JsTracerObject::evaluate(&self.script, &self.config, &mut self.ctx)?;
        // Callback objects are mutable JS objects: replacing their Rust state does not remove
        // user-defined properties or restore overwritten methods. Rebuild them once per
        // transaction, while retaining the parsed script and reusing wrappers within a transaction.
        let reusable_step_log = ReusableStepLog::new(&mut self.ctx, self.op_names.clone())
            .map_err(JsInspectorError::EvalCode)?;
        let reusable_call_frame =
            ReusableCallFrame::new(&mut self.ctx).map_err(JsInspectorError::EvalCode)?;
        let reusable_frame_result =
            ReusableFrameResult::new(&mut self.ctx).map_err(JsInspectorError::EvalCode)?;
        let reusable_db = ReusableEvmDb::new(&mut self.ctx).map_err(JsInspectorError::EvalCode)?;

        self.reusable_step_log = reusable_step_log;
        self.reusable_call_frame = reusable_call_frame;
        self.reusable_frame_result = reusable_frame_result;
        self.reusable_db = reusable_db;
        self.this = obj.clone().into();
        self.obj = obj;
        self.result_fn = result_fn;
        self.fault_fn = fault_fn;
        self.enter_fn = enter_fn;
        self.exit_fn = exit_fn;
        self.step_fn = step_fn;
        self.call_stack.clear();
        // call ids must stay unique across transactions, see `CallStackItem::id`
        self.precompiles_registered = false;
        self.step_pending = false;
        self.gas_spent_before = 0;
        Ok(())
    }

    /// Returns the transaction context.
    pub const fn transaction_context(&self) -> &TransactionContext {
        &self.transaction_context
    }

    /// Sets the transaction context.
    pub fn set_transaction_context(&mut self, transaction_context: TransactionContext) {
        self.transaction_context = transaction_context;
    }

    /// Applies the runtime limits to the JS context.
    ///
    /// By default
    pub fn set_runtime_limits(&mut self, limits: RuntimeLimits) {
        self.ctx.set_runtime_limits(limits);
    }

    /// Calls the result function and returns the result as [serde_json::Value].
    ///
    /// Note: This is supposed to be called after the inspection has finished.
    pub fn json_result<DB>(
        &mut self,
        res: ResultAndState<impl HaltReasonTr>,
        tx: &impl Transaction,
        block: &impl Block,
        db: &DB,
    ) -> Result<serde_json::Value, JsInspectorError>
    where
        DB: DatabaseRef,
        <DB as DatabaseRef>::Error: core::fmt::Display,
    {
        let result = self.result(res, tx, block, db)?;
        let result = to_serde_value(result, &mut self.ctx)?;
        self.ensure_not_interrupted()?;
        Ok(result)
    }

    /// Calls the result function and returns the result.
    pub fn result<TX, DB>(
        &mut self,
        res: ResultAndState<impl HaltReasonTr>,
        tx: &TX,
        block: &impl Block,
        db: &DB,
    ) -> Result<JsValue, JsInspectorError>
    where
        TX: Transaction,
        DB: DatabaseRef,
        <DB as DatabaseRef>::Error: core::fmt::Display,
    {
        self.ensure_not_interrupted()?;
        let ResultAndState { result, state } = res;
        let mut db = WrapDatabaseRef(db);

        let gas_used = result.tx_gas_used();
        let mut to = None;
        let mut output_bytes = None;
        let mut error = None;
        match result {
            ExecutionResult::Success { output, .. } => match output {
                Output::Call(out) => {
                    output_bytes = Some(out);
                }
                Output::Create(out, addr) => {
                    to = addr;
                    output_bytes = Some(out);
                }
            },
            ExecutionResult::Revert { output, .. } => {
                error = Some("execution reverted".to_string());
                output_bytes = Some(output);
            }
            ExecutionResult::Halt { reason, .. } => {
                error = Some(format!("execution halted: {reason:?}"));
            }
        };

        match tx.kind() {
            TransactTo::Call(target) => to = Some(target),
            // A failed creation has no output address, but its target is still determined by the
            // sender and nonce; report it rather than `null`.
            TransactTo::Create => to = to.or_else(|| Some(tx.caller().create(tx.nonce()))),
        }

        let ctx = JsEvmContext {
            r#type: match tx.kind() {
                TransactTo::Call(_) => "CALL",
                TransactTo::Create => "CREATE",
            }
            .to_string(),
            from: tx.caller(),
            to,
            input: tx.input().clone(),
            gas: tx.gas_limit(),
            gas_used,
            gas_price: tx
                .effective_gas_price(block.basefee() as u128)
                .try_into()
                .unwrap_or(u64::MAX),
            value: tx.value(),
            block: block.number().try_into().unwrap_or(u64::MAX),
            coinbase: block.beneficiary(),
            output: output_bytes.unwrap_or_default(),
            time: block.timestamp().to_string(),
            intrinsic_gas: 0,
            transaction_ctx: self.transaction_context,
            error,
        };
        let ctx = ctx.into_js_object(&mut self.ctx)?;
        let result = self.reusable_db.with_scope(&state, &mut db, || {
            self.result_fn.call(&self.this, &[ctx.into(), self.reusable_db.value()], &mut self.ctx)
        });
        self.ensure_not_interrupted()?;
        Ok(result?)
    }

    fn try_enter(&mut self, frame: CallFrame) -> JsResult<()> {
        if let Some(enter_fn) = &self.enter_fn {
            self.reusable_call_frame.update(frame);
            enter_fn.call(&self.this, &[self.reusable_call_frame.value()], &mut self.ctx)?;
        }
        Ok(())
    }

    fn try_exit(&mut self, frame: FrameResult) -> JsResult<()> {
        if let Some(exit_fn) = &self.exit_fn {
            self.reusable_frame_result.update(frame);
            exit_fn.call(&self.this, &[self.reusable_frame_result.value()], &mut self.ctx)?;
        }
        Ok(())
    }

    /// Returns the currently active call
    ///
    /// Panics: if there's no call yet
    #[track_caller]
    fn active_call(&self) -> &CallStackItem {
        self.call_stack.last().expect("call stack is empty")
    }

    #[inline]
    fn pop_call(&mut self) {
        self.call_stack.pop();
    }

    /// Returns true whether the active call is the root call.
    #[inline]
    fn is_root_call_active(&self) -> bool {
        self.call_stack.len() == 1
    }

    /// Returns true if there's an enter function and the active call is not the root call.
    #[inline]
    fn can_call_enter(&self) -> bool {
        self.enter_fn.is_some() && !self.is_root_call_active()
    }

    /// Returns true if there's an exit function and the active call is not the root call.
    #[inline]
    fn can_call_exit(&mut self) -> bool {
        self.exit_fn.is_some() && !self.is_root_call_active()
    }

    /// Pushes a new call to the stack
    fn push_call(
        &mut self,
        contract: Address,
        input: Bytes,
        value: U256,
        kind: CallKind,
        caller: Address,
        gas_limit: u64,
    ) -> &CallStackItem {
        let call = CallStackItem {
            id: self.next_call_id,
            contract: Contract { caller, contract, value, input },
            kind,
            gas_limit,
        };
        self.next_call_id += 1;
        self.call_stack.push(call);
        self.active_call()
    }

    /// Registers the precompiles in the JS context
    fn register_precompiles<CTX: ContextTr<Journal: JournalExt>>(&mut self, context: &mut CTX) {
        if self.precompiles_registered {
            return;
        }
        let precompiles =
            PrecompileList(context.journal().precompile_addresses().iter().copied().collect());

        let _ = precompiles.register_callable(&mut self.ctx);

        self.precompiles_registered = true
    }

    /// Sets a shared handle that cooperatively interrupts JavaScript tracing.
    ///
    /// Call [`JsInspectorInterrupt::interrupt`] from a timeout or cancellation handler to abort
    /// execution with a revm custom error at the next inspection boundary. [`Self::try_clone`]
    /// shares the handle and [`Self::fuse`] does not reset it. Result collection also fails after
    /// interruption.
    ///
    /// The handle is checked even if the tracer has no `step` callback. Checking it performs a
    /// relaxed atomic load without cloning the handle. Without a handle, no atomic loads are
    /// performed.
    ///
    /// This cannot preempt a running JavaScript callback, native operation or precompile. It does
    /// not bound memory usage or interrupt script evaluation and `setup` in the constructor.
    /// Boa's runtime limits still apply while JavaScript is running.
    ///
    /// ```
    /// use revm_inspectors::tracing::js::{JsInspector, JsInspectorInterrupt};
    ///
    /// let interrupt = JsInspectorInterrupt::new();
    /// let inspector = JsInspector::new(
    ///     "{ fault: function() {}, result: function() {} }".into(),
    ///     serde_json::Value::Null,
    /// )?
    /// .with_interrupt(interrupt.clone());
    /// // The request's timeout or cancellation handler can signal the blocking execution.
    /// interrupt.interrupt();
    /// # Ok::<(), revm_inspectors::tracing::js::JsInspectorError>(())
    /// ```
    #[must_use]
    pub fn with_interrupt(mut self, interrupt: JsInspectorInterrupt) -> Self {
        self.interrupt = Some(interrupt);
        self
    }

    #[inline]
    fn is_interrupted(&self) -> bool {
        self.interrupt.as_ref().is_some_and(JsInspectorInterrupt::is_interrupted)
    }

    fn ensure_not_interrupted(&self) -> Result<(), JsInspectorError> {
        if self.is_interrupted() {
            return Err(JsInspectorError::Interrupted);
        }
        Ok(())
    }

    #[inline]
    fn check_interrupt(&self, context: &mut impl ContextTr) -> bool {
        if !self.is_interrupted() {
            return false;
        }
        if context.error().is_ok() {
            *context.error() = Err(ContextError::Custom(JsInspectorError::Interrupted.to_string()));
        }
        true
    }

    fn interrupt_result(
        &self,
        context: &mut impl ContextTr,
        gas_limit: u64,
    ) -> Option<InterpreterResult> {
        self.check_interrupt(context).then(|| InterpreterResult {
            result: InstructionResult::FatalExternalError,
            output: Bytes::new(),
            gas: Gas::new(gas_limit),
        })
    }

    #[inline]
    fn check_and_halt(&mut self, context: &mut impl ContextTr, interp: &mut Interpreter) -> bool {
        if !self.check_interrupt(context) {
            return false;
        }
        self.step_pending = false;
        // Replace pending CALL/RETURN actions too, so cancellation cannot be caught by the EVM.
        interp.bytecode.action().take();
        interp.bytecode.reset_action();
        interp.halt_fatal();
        true
    }
}

impl<CTX> Inspector<CTX> for JsInspector
where
    CTX: ContextTr<Journal: JournalExt>,
{
    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if self.check_and_halt(context, interp) || self.step_fn.is_none() {
            return;
        }

        // The JS step callback expects the pre-execution stack and memory but the gas cost of the
        // opcode, which is only known after it executed. Instead of copying the stack and memory,
        // only the parts the opcode can overwrite are saved here and the callback is invoked in
        // `step_end`.
        self.gas_spent_before = interp.gas.total_gas_spent();
        self.step_pending = true;
        let memory = interp.memory.context_memory();
        self.reusable_step_log.record_pre_execution(PreStep {
            pc: interp.bytecode.pc() as u64,
            op: interp.bytecode.opcode(),
            gas_remaining: interp.gas.remaining(),
            refund: interp.gas.refunded() as u64,
            stack: interp.stack.data(),
            memory: &memory,
        });
    }

    fn step_end(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if self.check_and_halt(context, interp) {
            return;
        }
        let Some(step_fn) = &self.step_fn else {
            return;
        };
        if !core::mem::take(&mut self.step_pending) {
            return;
        }

        let result = interp.bytecode.action().as_ref().and_then(|a| a.instruction_result());
        let is_revert = result.is_some_and(|r| r.is_revert());

        // Compute the actual gas cost now that the opcode has executed
        let cost = interp.gas.total_gas_spent().saturating_sub(self.gas_spent_before);
        let depth = context.journal_ref().depth() as u64;
        let call = self.call_stack.last().expect("call stack is empty");
        let info = StepInfo {
            cost,
            depth,
            error: if is_revert { result.map(|result| format!("{result:?}")) } else { None },
            op: is_revert.then_some(opcode::REVERT),
            caller: interp.input.caller_address,
            contract: interp.input.target_address,
            value: call.contract.value,
            input: &call.contract.input,
            call_id: call.id,
        };

        let (db, state) = context.journal_mut().db_and_state_mut();
        let res = self.reusable_db.with_scope(state, db, || {
            self.reusable_step_log.with_scope(
                interp.stack.data(),
                interp.memory.context_memory(),
                info,
                || {
                    let args = [self.reusable_step_log.value(), self.reusable_db.value()];
                    let f = if is_revert { &self.fault_fn } else { step_fn };
                    f.call(&self.this, &args, &mut self.ctx)
                },
            )
        });

        if self.check_and_halt(context, interp) {
            return;
        }

        // Only set revert if the opcode didn't already set an action (e.g. STOP/RETURN).
        // If the opcode completed successfully, we can't revert it after the fact.
        if !is_revert && res.is_err() && interp.bytecode.action().is_none() {
            interp
                .bytecode
                .set_action(InterpreterAction::new_halt(InstructionResult::Revert, interp.gas));
        }
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        if let Some(result) = self.interrupt_result(context, inputs.gas_limit) {
            return Some(CallOutcome::new(result, inputs.return_memory_offset.clone()));
        }
        self.register_precompiles(context);

        // determine contract and caller based on the call scheme
        let (caller, contract) = match inputs.scheme {
            CallScheme::DelegateCall | CallScheme::CallCode => {
                (inputs.target_address, inputs.bytecode_address)
            }
            _ => (inputs.caller, inputs.target_address),
        };

        // A delegate call transfers nothing but inherits its parent's value, which revm keeps as
        // the apparent value; `transfer_value` alone would report it as zero.
        let value = inputs.transfer_value().or_else(|| inputs.apparent_value()).unwrap_or_default();
        self.push_call(
            contract,
            inputs.input_data(context),
            value,
            inputs.scheme.into(),
            caller,
            inputs.gas_limit,
        );

        let mut result = None;
        if self.can_call_enter() {
            let call = self.active_call();
            let frame = CallFrame {
                contract: call.contract.clone(),
                kind: FrameKind::Call(call.kind),
                gas: inputs.gas_limit,
            };
            result = self.try_enter(frame).err().map(js_error_to_revert);
        }

        self.interrupt_result(context, inputs.gas_limit)
            .or(result)
            .map(|result| CallOutcome::new(result, inputs.return_memory_offset.clone()))
    }

    fn call_end(&mut self, context: &mut CTX, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        if self.check_interrupt(context) {
            return;
        }
        if self.can_call_exit() {
            let frame_result = FrameResult {
                gas_used: outcome.result.gas.total_gas_spent(),
                output: outcome.result.output.clone(),
                error: utils::fmt_error_msg(outcome.result.result, TraceStyle::Geth),
            };
            if let Err(err) = self.try_exit(frame_result) {
                outcome.result = js_error_to_revert(err);
            }
        }

        self.pop_call();
        self.check_interrupt(context);
    }

    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        if let Some(result) = self.interrupt_result(context, inputs.gas_limit()) {
            return Some(CreateOutcome::new(result, None));
        }
        self.register_precompiles(context);

        let nonce = context.journal_mut().load_account(inputs.caller()).unwrap().info.nonce;
        let contract = inputs.created_address(nonce);
        self.push_call(
            contract,
            inputs.init_code().clone(),
            inputs.value(),
            inputs.scheme().into(),
            inputs.caller(),
            inputs.gas_limit(),
        );

        let mut result = None;
        if self.can_call_enter() {
            let call = self.active_call();
            let frame = CallFrame {
                contract: call.contract.clone(),
                kind: FrameKind::Call(call.kind),
                gas: call.gas_limit,
            };
            result = self.try_enter(frame).err().map(js_error_to_revert);
        }

        self.interrupt_result(context, inputs.gas_limit())
            .or(result)
            .map(|result| CreateOutcome::new(result, None))
    }

    fn create_end(
        &mut self,
        context: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        if self.check_interrupt(context) {
            return;
        }
        if self.can_call_exit() {
            let frame_result = FrameResult {
                gas_used: outcome.result.gas.total_gas_spent(),
                output: outcome.result.output.clone(),
                error: utils::fmt_error_msg(outcome.result.result, TraceStyle::Geth),
            };
            if let Err(err) = self.try_exit(frame_result) {
                outcome.result = js_error_to_revert(err);
            }
        }

        self.pop_call();
        self.check_interrupt(context);
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        if self.is_interrupted() {
            return;
        }
        // This is exempt from the root call constraint, because selfdestruct is treated as a
        // new scope that is entered and immediately exited.
        if self.enter_fn.is_some() {
            // The frame describes the destruction itself: the destroyed contract sends its balance
            // to the beneficiary, with no input and no gas. Reusing the enclosing call's frame
            // reported the outer caller and callee instead.
            let frame = CallFrame {
                contract: Contract {
                    caller: contract,
                    contract: target,
                    value,
                    input: Bytes::new(),
                },
                kind: FrameKind::SelfDestruct,
                gas: 0,
            };
            let _ = self.try_enter(frame);
        }

        // exit with empty frame result ref <https://github.com/ethereum/go-ethereum/blob/0004c6b229b787281760b14fb9460ffd9c2496f1/core/vm/instructions.go#L829-L829>
        if !self.is_interrupted() && self.exit_fn.is_some() {
            let frame_result = FrameResult { gas_used: 0, output: Bytes::new(), error: None };
            let _ = self.try_exit(frame_result);
        }
    }
}

/// A shared handle for cooperatively interrupting a [`JsInspector`].
///
/// Clones share the same signal. Interruption is permanent and dropping a handle does not
/// interrupt execution. The signal uses relaxed atomics and does not synchronize other data.
/// Use [`Self::drop_guard`] to interrupt execution when the caller is dropped.
#[derive(Clone, Debug, Default)]
pub struct JsInspectorInterrupt(Arc<AtomicBool>);

impl JsInspectorInterrupt {
    /// Creates a handle that has not been interrupted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Signals interruption to all inspectors sharing this handle.
    #[inline]
    pub fn interrupt(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Returns whether interruption has been requested.
    #[inline]
    pub fn is_interrupted(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Creates an owned guard that interrupts execution when dropped.
    ///
    /// The guard shares this handle's signal. Keep it in the request future while the inspector
    /// runs on a blocking worker, so dropping the request also interrupts tracing. Create the
    /// guard before moving it into the future to cover cancellation before the first poll.
    ///
    /// ```
    /// use revm_inspectors::tracing::js::JsInspectorInterrupt;
    ///
    /// let interrupt = JsInspectorInterrupt::new();
    /// let guard = interrupt.drop_guard();
    /// assert!(!interrupt.is_interrupted());
    /// drop(guard);
    /// assert!(interrupt.is_interrupted());
    /// ```
    pub fn drop_guard(&self) -> JsInspectorInterruptGuard {
        JsInspectorInterruptGuard(self.clone())
    }
}

/// A guard that signals its shared interrupt when dropped.
///
/// Created by [`JsInspectorInterrupt::drop_guard`]. Dropping any guard interrupts all inspectors
/// sharing the signal, even if other handles or guards remain alive.
#[derive(Debug)]
#[must_use = "the guard interrupts tracing immediately if it is not retained"]
pub struct JsInspectorInterruptGuard(JsInspectorInterrupt);

impl Drop for JsInspectorInterruptGuard {
    fn drop(&mut self) {
        self.0.interrupt();
    }
}

/// The evaluated tracer object and its callback functions.
struct JsTracerObject {
    obj: JsObject,
    result_fn: JsObject,
    fault_fn: JsObject,
    enter_fn: Option<JsObject>,
    exit_fn: Option<JsObject>,
    step_fn: Option<JsObject>,
}

impl JsTracerObject {
    /// Evaluates the script to a fresh tracer object, validates its callbacks and invokes `setup`.
    fn evaluate(
        script: &Script,
        config: &serde_json::Value,
        ctx: &mut Context,
    ) -> Result<Self, JsInspectorError> {
        let obj = script.evaluate(ctx).map_err(JsInspectorError::EvalCode)?;
        let obj = obj.as_object().ok_or(JsInspectorError::ExpectedJsObject)?;

        // ensure all the fields are callables, if present

        let result_fn = obj
            .get(js_string!("result"), ctx)?
            .as_object()
            .ok_or(JsInspectorError::ResultFunctionMissing)?;
        if !result_fn.is_callable() {
            return Err(JsInspectorError::ResultFunctionMissing);
        }

        let fault_fn = obj
            .get(js_string!("fault"), ctx)?
            .as_object()
            .ok_or(JsInspectorError::FaultFunctionMissing)?;
        if !fault_fn.is_callable() {
            return Err(JsInspectorError::FaultFunctionMissing);
        }

        let enter_fn = obj.get(js_string!("enter"), ctx)?.as_object().filter(|o| o.is_callable());
        let exit_fn = obj.get(js_string!("exit"), ctx)?.as_object().filter(|o| o.is_callable());
        let step_fn = obj.get(js_string!("step"), ctx)?.as_object().filter(|o| o.is_callable());

        let js_config_value =
            JsValue::from_json(config, ctx).map_err(JsInspectorError::InvalidJsonConfig)?;

        if let Some(setup_fn) = obj.get(js_string!("setup"), ctx)?.as_object() {
            if !setup_fn.is_callable() {
                return Err(JsInspectorError::SetupFunctionNotCallable);
            }

            // call setup()
            setup_fn
                .call(&(obj.clone().into()), core::slice::from_ref(&js_config_value), ctx)
                .map_err(JsInspectorError::SetupCallFailed)?;
        }

        Ok(Self { obj, result_fn, fault_fn, enter_fn, exit_fn, step_fn })
    }
}

/// Represents an active call
#[derive(Debug)]
struct CallStackItem {
    /// Unique id of the call within the inspector, used to detect call changes between steps.
    id: u64,
    contract: Contract,
    kind: CallKind,
    gas_limit: u64,
}

/// Error variants that can occur during JavaScript inspection.
#[derive(Debug, thiserror::Error)]
pub enum JsInspectorError {
    /// Error originating from a JavaScript operation.
    #[error(transparent)]
    JsError(#[from] JsError),

    /// Failure during the evaluation of JavaScript code.
    #[error("failed to evaluate JS code: {0}")]
    EvalCode(JsError),

    /// The evaluated code is not a JavaScript object.
    #[error("the evaluated code is not a JS object")]
    ExpectedJsObject,

    /// The trace object must expose a function named `result()`.
    #[error("trace object must expose a function result()")]
    ResultFunctionMissing,

    /// The trace object must expose a function named `fault()`.
    #[error("trace object must expose a function fault()")]
    FaultFunctionMissing,

    /// The setup object must be a callable function.
    #[error("setup object must be a function")]
    SetupFunctionNotCallable,

    /// Failure during the invocation of the `setup()` function.
    #[error("failed to call setup(): {0}")]
    SetupCallFailed(JsError),

    /// Invalid JSON configuration encountered.
    #[error("invalid JSON config: {0}")]
    InvalidJsonConfig(JsError),

    /// Tracing was cancelled through the shared interrupt flag.
    #[error("JavaScript tracing interrupted")]
    Interrupted,
}

/// Converts a JavaScript error into a [InstructionResult::Revert] [InterpreterResult].
#[inline]
fn js_error_to_revert(err: JsError) -> InterpreterResult {
    let output = err.to_string().as_bytes().to_vec();
    InterpreterResult { result: InstructionResult::Revert, output: output.into(), gas: Gas::new(0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    use alloy_primitives::{bytes, hex, Address};
    use revm::{
        context::TxEnv,
        database::CacheDB,
        database_interface::EmptyDB,
        inspector::InspectorEvmTr,
        primitives::hardfork::SpecId,
        state::{AccountInfo, Bytecode},
        InspectEvm, MainBuilder, MainContext,
    };
    //use revm_inspector::{inspector_handler, InspectorContext, InspectorMainEvm};
    use serde_json::json;

    #[test]
    fn test_loop_iteration_limit() {
        let mut context = Context::default();
        context.runtime_limits_mut().set_loop_iteration_limit(LOOP_ITERATION_LIMIT);

        let code = "let i = 0; while (i++ < 69) {}";
        let result = context.eval(Source::from_bytes(code));
        assert!(result.is_ok());

        let code = "while (true) {}";
        let result = context.eval(Source::from_bytes(code));
        assert!(result.is_err());
    }

    #[test]
    fn test_fault_fn_not_callable() {
        let code = r#"
            {
                result: function() {},
                fault: {},
            }
        "#;
        let config = serde_json::Value::Null;
        let result = JsInspector::new(code.to_string(), config);
        assert!(matches!(result, Err(JsInspectorError::FaultFunctionMissing)));
    }

    // Helper function to run a trace and return the result
    fn run_trace(code: &str, contract: Option<Bytes>, success: bool) -> serde_json::Value {
        let addr = Address::repeat_byte(0x01);
        let mut db = CacheDB::new(EmptyDB::default());

        // Insert the caller
        db.insert_account_info(
            Address::ZERO,
            AccountInfo { balance: U256::from(1e18), ..Default::default() },
        );
        // Insert the contract
        db.insert_account_info(
            addr,
            AccountInfo {
                code: Some(Bytecode::new_legacy(
                    /* PUSH1 1, PUSH1 1, STOP */
                    contract.unwrap_or_else(|| hex!("6001600100").into()),
                )),
                ..Default::default()
            },
        );

        let insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();

        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db)
            .build_mainnet_with_inspector(insp);

        let res = evm
            .inspect_tx(TxEnv {
                gas_price: 1024,
                gas_limit: 1_000_000,
                gas_priority_fee: None,
                kind: TransactTo::Call(addr),
                ..Default::default()
            })
            .expect("pass without error");

        assert_eq!(res.result.is_success(), success);
        let (ctx, inspector) = evm.ctx_inspector();
        let tx = ctx.tx().clone();
        let block = ctx.block().clone();
        inspector.json_result(res, &tx, &block, ctx.db_mut()).unwrap()
    }

    #[test]
    fn test_general_counting() {
        let code = r#"{
            count: 0,
            step: function() { this.count += 1; },
            fault: function() {},
            result: function() { return this.count; }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res.as_u64().unwrap(), 3);
    }

    #[test]
    fn test_memory_access() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.memory.slice(-1,-2)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_memory_slice_rejects_non_finite_indexes() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.memory.slice(Infinity, NaN)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_memory_slice_rejects_non_finite_end() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.memory.slice(0, Infinity)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_memory_slice_accepts_bigint_index() {
        let code = r#"{
            res: [],
            step: function(log) { this.res.push(log.memory.slice(0, 0n)); },
            fault: function() {},
            result: function() { return this.res; }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res, json!([json!({}), json!({}), json!({})]));
    }

    #[test]
    fn test_memory_slice_rejects_bigint_index_overflow() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.memory.slice(0, 340282366920938463463374607431768211455n)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_stack_peek() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.stack.peek(-1)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_stack_peek_nan() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.stack.peek(NaN)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_stack_peek_infinity() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.stack.peek(Infinity)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_memory_get_uint() {
        let code = r#"{
            depths: [],
            step: function(log, db) { this.depths.push(log.memory.getUint(-64)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_memory_get_uint_rejects_non_finite_offset() {
        let code = r#"{
            depths: [],
            step: function(log, db) { this.depths.push(log.memory.getUint(Infinity)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_memory_get_uint_rejects_nan_offset() {
        let code = r#"{
            depths: [],
            step: function(log, db) { this.depths.push(log.memory.getUint(NaN)); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, false);
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_stack_depth() {
        let code = r#"{
            depths: [],
            step: function(log) { this.depths.push(log.stack.length()); },
            fault: function() {},
            result: function() { return this.depths; }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res, json!([0, 1, 2]));
    }

    #[test]
    fn test_memory_length() {
        let code = r#"{
            lengths: [],
            step: function(log) { this.lengths.push(log.memory.length()); },
            fault: function() {},
            result: function() { return this.lengths; }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res, json!([0, 0, 0]));
    }

    #[test]
    fn test_opcode_to_string() {
        let code = r#"{
             opcodes: [],
             step: function(log) { this.opcodes.push(log.op.toString()); },
             fault: function() {},
             result: function() { return this.opcodes; }
         }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res, json!(["PUSH1", "PUSH1", "STOP"]));
    }

    #[test]
    fn test_gas_used() {
        let code = r#"{
            depths: [],
            step: function() {},
            fault: function() {},
            result: function(ctx) { return ctx.gasPrice+'.'+ctx.gasUsed; }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res.as_str().unwrap(), "1024.21006");
    }

    #[test]
    fn test_to_word() {
        let code = r#"{
            res: null,
            step: function(log) {},
            fault: function() {},
            result: function() { return toWord('0xffaa') }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(
            res,
            json!({
                "0": 0, "1": 0, "2": 0, "3": 0, "4": 0, "5": 0, "6": 0, "7": 0, "8": 0,
                "9": 0, "10": 0, "11": 0, "12": 0, "13": 0, "14": 0, "15": 0, "16": 0,
                "17": 0, "18": 0, "19": 0, "20": 0, "21": 0, "22": 0, "23": 0, "24": 0,
                "25": 0, "26": 0, "27": 0, "28": 0, "29": 0, "30": 255, "31": 170,
            })
        );
    }

    #[test]
    fn test_to_address() {
        let code = r#"{
            res: null,
            step: function(log) { var address = log.contract.getAddress(); this.res = toAddress(address); },
            fault: function() {},
            result: function() { return toHex(this.res) }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res.as_str().unwrap(), "0x0101010101010101010101010101010101010101");
    }

    #[test]
    fn test_to_address_string() {
        let code = r#"{
            res: null,
            step: function(log) { var address = '0x0000000000000000000000000000000000000000'; this.res = toAddress(address); },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res.as_object().unwrap().values().map(|v| v.as_u64().unwrap()).sum::<u64>(), 0);
    }

    #[test]
    fn test_memory_slice() {
        let code = r#"{
            res: [],
            step: function(log) {
                var op = log.op.toString();
                if (op === 'MSTORE8' || op === 'STOP') {
                    this.res.push(log.memory.slice(0, 2))
                }
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let contract = hex!("60ff60005300"); // PUSH1, 0xff, PUSH1, 0x00, MSTORE8, STOP
        let res = run_trace(code, Some(contract.into()), false);
        assert_eq!(res, json!([]));
    }

    #[test]
    fn test_memory_limit() {
        // Accessing out-of-bounds memory in the tracer results in an empty array.
        // Since we invoke the JS step callback in step_end (after the opcode executes),
        // a JS error on the final STOP opcode cannot revert the transaction — it has
        // already completed. The transaction succeeds but the trace result is empty.
        let code = r#"{
            res: [],
            step: function(log) { if (log.op.toString() === 'STOP') { this.res.push(log.memory.slice(5, 1025 * 1024)) } },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res, json!([]));
    }

    #[test]
    fn test_coinbase() {
        let code = r#"{
            lengths: [],
            step: function(log) { },
            fault: function() {},
            result: function(ctx) { var coinbase = ctx.coinbase; return toAddress(coinbase); }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res.as_object().unwrap().values().map(|v| v.as_u64().unwrap()).sum::<u64>(), 0);
    }

    #[test]
    fn test_individual_opcode_costs() {
        let code = r#"{
            res: [],
            step: function(log) {
                this.res.push(log.getCost());
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, None, true);

        // The bytecode is: PUSH1 0x01, PUSH1 0x01, STOP
        // Expected costs: PUSH1=3, PUSH1=3, STOP=0
        assert_eq!(
            res.as_array().unwrap().iter().map(|v| v.as_u64().unwrap_or(0)).collect::<Vec<u64>>(),
            vec![3, 3, 0]
        );
    }

    #[test]
    fn test_slice_builtin() {
        let code = r#"{
            res: [],
            step: function(log) {
                // Test slicing a hex string
                var hex = '0xdeadbeefcafe';
                this.res.push(toHex(slice(hex, 0, 2)));
                this.res.push(toHex(slice(hex, 2, 4)));
                this.res.push(toHex(slice(hex, 4, 6)));

                // Test slicing an array
                var arr = [0x01, 0x02, 0x03, 0x04, 0x05];
                this.res.push(toHex(slice(arr, 0, 3)));
                this.res.push(toHex(slice(arr, 1, 4)));

                // Test slicing a Uint8Array
                var uint8 = new Uint8Array([0xff, 0xee, 0xdd, 0xcc, 0xbb]);
                this.res.push(toHex(slice(uint8, 0, 2)));
                this.res.push(toHex(slice(uint8, 2, 5)));
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, Some(bytes!("0x00")), true);
        assert_eq!(
            res,
            json!(["0xdead", "0xbeef", "0xcafe", "0x010203", "0x020304", "0xffee", "0xddccbb"])
        );
    }

    #[test]
    fn test_is_precompiled_builtin() {
        let code = r#"{
            res: [],
            step: function(log) {
                this.res.push(isPrecompiled("0x01"));
                this.res.push(isPrecompiled("0x0000000000000000000000000000000000000002"));
                this.res.push(isPrecompiled("0x0000000000000000000000000000000000000000"));
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, Some(bytes!("0x00")), true);
        assert_eq!(res, json!([true, true, false]));
    }

    #[test]
    fn test_has_own_property() {
        let code = r#"{
            res: [],
            step: function(log) {
                this.res.push(log.hasOwnProperty("stack"));
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, Some(bytes!("0x00")), true);
        assert_eq!(res, json!([true]));
    }

    #[test]
    fn test_step_reuses_log_and_db_objects() {
        let code = r#"{
            prevLog: null,
            prevDb: null,
            sameLog: [],
            sameDb: [],
            step: function(log, db) {
                if (this.prevLog !== null) {
                    this.sameLog.push(this.prevLog === log);
                }
                if (this.prevDb !== null) {
                    this.sameDb.push(this.prevDb === db);
                }
                this.prevLog = log;
                this.prevDb = db;
            },
            fault: function() {},
            result: function() {
                return {
                    sameLog: this.sameLog,
                    sameDb: this.sameDb,
                };
            }
        }"#;
        let res = run_trace(code, None, true);
        assert_eq!(res, json!({ "sameLog": [true, true], "sameDb": [true, true] }));
    }

    #[test]
    fn test_slice_with_stack_values() {
        let code = r#"{
            res: [],
            step: function(log) {
                if ((log.stack.length() > 0) && log.memory.length() >= log.stack.peek(0)) {
                    this.res.push(log.memory.slice(0, log.stack.peek(0)));
                }
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, Some(bytes!("0x5F5F52600100")), true);
        assert_eq!(res, json!([json!({}), json!({}), json!({"0": 0})]));
    }

    #[test]
    fn test_step_sees_pre_execution_stack() {
        let code = r#"{
            res: [],
            step: function(log) {
                if (log.op.toString() === 'ADD') {
                    this.res.push(log.stack.length());
                    this.res.push(log.stack.peek(0));
                    this.res.push(log.stack.peek(1));
                }
                if (log.op.toString() === 'STOP') {
                    this.res.push(log.stack.length());
                    this.res.push(log.stack.peek(0));
                }
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        // PUSH1 1, PUSH1 2, ADD, STOP
        let res = run_trace(code, Some(bytes!("0x600160020100")), true);
        assert_eq!(res, json!([2, "2", "1", 1, "3"]));
    }

    #[test]
    fn test_step_sees_pre_execution_memory() {
        let code = r#"{
            res: [],
            step: function(log) {
                var op = log.op.toString();
                if (op === 'MSTORE8' || op === 'STOP') {
                    this.res.push(log.memory.length());
                    if (log.memory.length() > 0) {
                        this.res.push(log.memory.getUint(0)[0]);
                        this.res.push(log.memory.slice(0, 2)[0]);
                    }
                }
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        // PUSH1 0xff, PUSH1 0, MSTORE8, PUSH1 0xaa, PUSH1 0, MSTORE8, STOP
        let res = run_trace(code, Some(bytes!("0x60ff60005360aa60005300")), true);
        // the second MSTORE8 must still see the value written by the first one
        assert_eq!(res, json!([0, 32, 255, 255, 32, 170, 170]));
    }

    #[test]
    fn test_fuse_resets_tracer() {
        let code = r#"{
            count: 0,
            step: function() { this.count += 1; },
            fault: function() {},
            result: function() { return this.count; }
        }"#;
        let addr = Address::repeat_byte(0x01);
        let mut db = CacheDB::new(EmptyDB::default());
        db.insert_account_info(
            Address::ZERO,
            AccountInfo { balance: U256::from(1e18), ..Default::default() },
        );
        db.insert_account_info(
            addr,
            AccountInfo {
                code: Some(Bytecode::new_legacy(hex!("6001600100").into())),
                ..Default::default()
            },
        );

        let insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db)
            .build_mainnet_with_inspector(insp);

        for _ in 0..2 {
            let res = evm
                .inspect_tx(TxEnv {
                    gas_price: 1024,
                    gas_limit: 1_000_000,
                    kind: TransactTo::Call(addr),
                    ..Default::default()
                })
                .unwrap();
            let (ctx, inspector) = evm.ctx_inspector();
            let tx = ctx.tx().clone();
            let block = ctx.block().clone();
            let count = inspector.json_result(res, &tx, &block, ctx.db_mut()).unwrap();
            assert_eq!(count.as_u64().unwrap(), 3);
            inspector.fuse().unwrap();
        }
    }

    #[test]
    fn test_fuse_resets_callback_objects() {
        let code = r#"{
            counts: { log: 0, db: 0, enter: 0, exit: 0 },
            setup: function(config) {
                if (config.used) throw new Error("config was reused");
                config.used = true;
                this.ready = true;
            },
            mark: function(object, kind) {
                if (!this.ready) throw new Error("setup was not called");
                if (!object.seen) {
                    // Non-configurable properties cannot be removed by clearing the wrapper.
                    Object.defineProperty(object, "seen", { value: true });
                    this.counts[kind]++;
                }
            },
            step: function(log, db) {
                this.mark(log, "log");
                this.mark(db, "db");
            },
            enter: function(frame) { this.mark(frame, "enter"); },
            exit: function(result) { this.mark(result, "exit"); },
            fault: function() {},
            result: function() { return this.counts; }
        }"#;
        let addr = Address::repeat_byte(0x01);
        let child = Address::repeat_byte(0x02);
        // Call the child twice to also verify that wrappers are reused within a transaction.
        let mut bytecode = Vec::new();
        for _ in 0..2 {
            bytecode.extend_from_slice(&hex!("60006000600060006000"));
            bytecode.push(0x73); // PUSH20
            bytecode.extend_from_slice(child.as_slice());
            bytecode.extend_from_slice(&hex!("61fffff150")); // PUSH2 gas, CALL, POP
        }
        bytecode.push(0x00);
        let mut db = CacheDB::new(EmptyDB::default());
        db.insert_account_info(
            addr,
            AccountInfo { code: Some(Bytecode::new_legacy(bytecode.into())), ..Default::default() },
        );
        db.insert_account_info(
            child,
            AccountInfo {
                code: Some(Bytecode::new_legacy(hex!("600100").into())),
                ..Default::default()
            },
        );
        let inspector = JsInspector::new(code.to_string(), json!({})).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db)
            .build_mainnet_with_inspector(inspector);

        for _ in 0..3 {
            let res = evm
                .inspect_tx(TxEnv {
                    gas_limit: 1_000_000,
                    kind: TransactTo::Call(addr),
                    ..Default::default()
                })
                .unwrap();
            assert!(res.result.is_success());
            let (ctx, inspector) = evm.ctx_inspector();
            let tx = ctx.tx().clone();
            let block = ctx.block().clone();
            let counts = inspector.json_result(res, &tx, &block, ctx.db_mut()).unwrap();
            assert_eq!(counts, json!({"log": 1, "db": 1, "enter": 1, "exit": 1}));
            inspector.fuse().unwrap();
        }
    }

    #[test]
    fn test_bigint_survives_poisoned_global() {
        let code = r#"{
            res: {},
            step: function(log, db) {
                // Poison the global bigint alias
                Object.defineProperty(globalThis, 'bigint', {
                    get() { throw new Error('poisoned bigint'); },
                    configurable: true
                });

                if (log.stack.length() > 0) {
                    // stack.peek internally uses to_bigint
                    this.res.stackPeek = log.stack.peek(0).toString();
                }
                // contract.getValue internally uses to_bigint
                this.res.value = log.contract.getValue().toString();
                // db.getBalance internally uses to_bigint
                this.res.balance = db.getBalance(log.contract.getAddress()).toString();
            },
            fault: function() {},
            result: function() { return this.res }
        }"#;
        let res = run_trace(code, None, true);
        let obj = res.as_object().unwrap();
        assert_eq!(obj["stackPeek"], json!("1"));
        assert_eq!(obj["value"], json!("0"));
        assert_eq!(obj["balance"], json!("0"));
    }

    /// Runs a tracer over `tx` against `db`, returning the tracer's result.
    fn trace_tx(code: &str, db: CacheDB<EmptyDB>, tx: TxEnv) -> serde_json::Value {
        let insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db)
            .build_mainnet_with_inspector(insp);
        let res = evm.inspect_tx(tx).expect("pass without error");
        let (ctx, inspector) = evm.ctx_inspector();
        let tx = ctx.tx().clone();
        let block = ctx.block().clone();
        inspector.json_result(res, &tx, &block, ctx.db_mut()).unwrap()
    }

    fn funded_db() -> CacheDB<EmptyDB> {
        let mut db = CacheDB::new(EmptyDB::default());
        db.insert_account_info(
            Address::ZERO,
            AccountInfo { balance: U256::from(1e18), ..Default::default() },
        );
        db
    }

    #[test]
    fn test_selfdestruct_frame_describes_the_destruction() {
        let contract = Address::repeat_byte(0xaa);
        let beneficiary = Address::repeat_byte(0xcc);
        let mut db = funded_db();
        // PUSH20 <beneficiary>, SELFDESTRUCT
        let mut code = vec![0x73];
        code.extend_from_slice(beneficiary.as_slice());
        code.push(0xff);
        db.insert_account_info(
            contract,
            AccountInfo {
                balance: U256::from(5),
                code: Some(Bytecode::new_legacy(code.into())),
                ..Default::default()
            },
        );

        let tracer = r#"{f:[],step:function(){},fault:function(){},enter:function(fr){this.f.push([fr.getType(),toHex(fr.getFrom()),toHex(fr.getTo()),fr.getValue().toString(),fr.getGas(),toHex(fr.getInput())])},exit:function(){},result:function(){return this.f}}"#;
        let res = trace_tx(
            tracer,
            db,
            TxEnv { gas_limit: 1_000_000, kind: TransactTo::Call(contract), ..Default::default() },
        );
        assert_eq!(
            res,
            json!([[
                "SELFDESTRUCT",
                format!("{contract:#x}"),
                format!("{beneficiary:#x}"),
                "5",
                0,
                "0x"
            ]])
        );
    }

    #[test]
    fn test_delegate_call_frame_reports_the_inherited_value() {
        let outer = Address::repeat_byte(0x01);
        let target = Address::repeat_byte(0x02);
        let mut db = funded_db();
        // PUSH1 0 x4, PUSH20 <target>, GAS, DELEGATECALL, STOP
        let mut code = hex!("6000600060006000").to_vec();
        code.push(0x73);
        code.extend_from_slice(target.as_slice());
        code.extend_from_slice(&hex!("5af400"));
        db.insert_account_info(
            outer,
            AccountInfo { code: Some(Bytecode::new_legacy(code.into())), ..Default::default() },
        );
        db.insert_account_info(
            target,
            AccountInfo {
                code: Some(Bytecode::new_legacy(hex!("00").into())),
                ..Default::default()
            },
        );

        let tracer = r#"{f:[],step:function(){},fault:function(){},enter:function(fr){this.f.push([fr.getType(),fr.getValue().toString()])},exit:function(){},result:function(){return this.f}}"#;
        let res = trace_tx(
            tracer,
            db,
            TxEnv {
                gas_limit: 1_000_000,
                value: U256::from(7),
                kind: TransactTo::Call(outer),
                ..Default::default()
            },
        );
        assert_eq!(res, json!([["DELEGATECALL", "7"]]));
    }

    #[test]
    fn test_ctx_to_is_set_for_a_failed_creation() {
        // Init code that reverts immediately: PUSH1 0, PUSH1 0, REVERT.
        let tracer = r#"{step:function(){},fault:function(){},result:function(ctx){return {to:toHex(ctx.to),err:ctx.error}}}"#;
        let res = trace_tx(
            tracer,
            funded_db(),
            TxEnv {
                gas_limit: 1_000_000,
                kind: TransactTo::Create,
                data: hex!("60006000fd").into(),
                ..Default::default()
            },
        );
        assert_eq!(res["to"], json!(format!("{:#x}", Address::ZERO.create(0))));
        assert_eq!(res["err"], json!("execution reverted"));
    }
}
