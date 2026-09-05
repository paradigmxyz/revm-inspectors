//! Type bindings for js tracing inspector

use crate::tracing::{
    js::builtins::{
        address_from_value, address_to_uint8_array, address_to_uint8_array_value, b256_from_value,
        call_kind_js_string, to_bigint, to_uint8_array, to_uint8_array_value,
        uint8_array_from_block,
    },
    types::CallKind,
    TransactionContext,
};
use alloc::{
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use boa_engine::{
    js_string,
    native_function::NativeFunction,
    object::{
        builtins::{AlignedVec, JsFunction},
        FunctionObjectBuilder,
    },
    Context, JsArgs, JsError, JsNativeError, JsObject, JsResult, JsString, JsValue,
};
use boa_gc::{empty_trace, Finalize, Trace};
use core::{
    cell::{Ref, RefCell, RefMut},
    ops::Range,
};
use revm::{
    bytecode::opcode::{self, OpCode, PUSH0, PUSH32},
    primitives::KECCAK_EMPTY,
    state::{AccountInfo, Bytecode, EvmState},
    Database,
};

/// Shared mutable state captured by JS native functions.
#[derive(Debug)]
struct Shared<T>(Rc<RefCell<T>>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> Shared<T> {
    fn new(value: T) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }

    fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}

impl<T> Finalize for Shared<T> {}

unsafe impl<T> Trace for Shared<T> {
    empty_trace!();
}

/// A native function body that operates on the shared state of the object it belongs to.
type StateFnPtr<T> = fn(&Shared<T>, &[JsValue], &mut Context) -> JsResult<JsValue>;

/// Wrapper so a function pointer can be captured by a boa native function.
struct StateFn<T>(StateFnPtr<T>);

impl<T> Clone for StateFn<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for StateFn<T> {}

impl<T> Finalize for StateFn<T> {}

unsafe impl<T> Trace for StateFn<T> {
    empty_trace!();
}

/// Builds a native JS function with access to the given shared state.
fn state_fn<T: 'static>(
    ctx: &mut Context,
    state: Shared<T>,
    length: usize,
    f: StateFnPtr<T>,
) -> JsFunction {
    FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, (state, f), ctx| (f.0)(state, args, ctx),
            (state, StateFn(f)),
        ),
    )
    .length(length)
    .build()
}

fn type_error(message: String) -> JsError {
    JsError::from_native(JsNativeError::typ().with_message(message))
}

/// Reusable JS log object for opcode steps.
///
/// The object graph (`log.op`, `log.stack`, `log.memory`, `log.contract` and the getters) is built
/// once, all callbacks read from a shared state that is refreshed for every step.
///
/// The stack and memory are not copied per step. [`Self::record_pre_execution`] saves the parts an
/// opcode can overwrite (its stack inputs and the memory range it writes) before it executes, and
/// [`Self::enter`] combines them with the post-execution stack and memory into the pre-execution
/// view that geth exposes to `step`.
#[derive(Debug)]
pub(crate) struct ReusableStepLog {
    state: Shared<StepLogState>,
    object: JsObject,
}

impl ReusableStepLog {
    pub(crate) fn new(ctx: &mut Context) -> JsResult<Self> {
        let state = Shared::new(StepLogState::new());
        let object = JsObject::with_object_proto(ctx.intrinsics());

        object.set(js_string!("op"), build_step_op_object(state.clone(), ctx)?, false, ctx)?;
        object.set(
            js_string!("memory"),
            build_step_memory_object(state.clone(), ctx)?,
            false,
            ctx,
        )?;
        object.set(
            js_string!("stack"),
            build_step_stack_object(state.clone(), ctx)?,
            false,
            ctx,
        )?;
        object.set(
            js_string!("contract"),
            build_step_contract_object(state.clone(), ctx)?,
            false,
            ctx,
        )?;

        let get_pc = state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().pc.into()));
        let get_gas =
            state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().gas_remaining.into()));
        let get_cost =
            state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().cost.into()));
        let get_depth =
            state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().depth.into()));
        let get_refund =
            state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().refund.into()));
        let get_error = state_fn(ctx, state.clone(), 0, |state, _, _| {
            Ok(state
                .borrow()
                .error
                .as_ref()
                .map(|error| JsValue::from(js_string!(error.as_str())))
                .unwrap_or_else(JsValue::undefined))
        });

        object.set(js_string!("getPC"), get_pc, false, ctx)?;
        object.set(js_string!("getError"), get_error, false, ctx)?;
        object.set(js_string!("getGas"), get_gas, false, ctx)?;
        object.set(js_string!("getCost"), get_cost, false, ctx)?;
        object.set(js_string!("getDepth"), get_depth, false, ctx)?;
        object.set(js_string!("getRefund"), get_refund, false, ctx)?;

        Ok(Self { state, object })
    }

    /// Records everything about a step that must be captured before the opcode executes.
    pub(crate) fn record_pre_execution(&self, step: PreStep<'_>) {
        let PreStep { pc, op, gas_remaining, refund, stack, memory } = step;
        let mut state = self.state.borrow_mut();
        state.pc = pc;
        state.op = op;
        state.gas_remaining = gas_remaining;
        state.refund = refund;
        state.stack.record(op, stack);
        state.memory.record(op, stack, memory);
    }

    /// Makes the post-execution `stack` and `memory` available to the JS object, so it can serve
    /// the pre-execution view recorded by [`Self::record_pre_execution`], and fills in the
    /// remaining step data.
    ///
    /// The access is revoked again when the returned guard is dropped, which must happen before
    /// the interpreter the values are borrowed from is used again.
    pub(crate) fn enter<'a>(
        &'a self,
        stack: &[U256],
        memory: Ref<'_, [u8]>,
        info: StepInfo<'_>,
    ) -> StepScope<'a> {
        // SAFETY: boa requires 'static values, the guard removes the references from the shared
        // state when it is dropped and JS code only runs while the guard is alive.
        let stack: &'static [U256] =
            unsafe { core::mem::transmute::<&[U256], &'static [U256]>(stack) };
        let memory: Ref<'static, [u8]> =
            unsafe { core::mem::transmute::<Ref<'_, [u8]>, Ref<'static, [u8]>>(memory) };
        let memory_slice: &'static [u8] =
            unsafe { core::mem::transmute::<&[u8], &'static [u8]>(&memory) };

        {
            let mut state = self.state.borrow_mut();
            state.stack.post = Some(stack);
            state.memory.post = Some(memory_slice);
            state.cost = info.cost;
            state.depth = info.depth;
            state.error = info.error;
            if let Some(op) = info.op {
                state.op = op;
            }
            state.contract.caller = info.caller;
            state.contract.contract = info.contract;
            state.contract.value = info.value;
            // the input only changes with the call, so it is only cloned once per call
            if state.call_id != info.call_id {
                state.call_id = info.call_id;
                state.contract.input = info.input.clone();
            }
        }

        StepScope { state: &self.state, _memory: memory }
    }

    pub(crate) fn value(&self) -> JsValue {
        self.object.clone().into()
    }
}

/// Pre-execution data of a step, see [`ReusableStepLog::record_pre_execution`].
#[derive(Debug)]
pub(crate) struct PreStep<'a> {
    /// Program counter before step execution
    pub(crate) pc: u64,
    /// Opcode to be executed
    pub(crate) op: u8,
    /// Remaining gas before step execution
    pub(crate) gas_remaining: u64,
    /// Gas refund counter before step execution
    pub(crate) refund: u64,
    /// Stack before step execution
    pub(crate) stack: &'a [U256],
    /// Memory before step execution
    pub(crate) memory: &'a [u8],
}

/// Post-execution data of a step, see [`ReusableStepLog::enter`].
#[derive(Debug)]
pub(crate) struct StepInfo<'a> {
    /// Gas cost of step execution
    pub(crate) cost: u64,
    /// Call depth
    pub(crate) depth: u64,
    /// Information about the error if one occurred
    pub(crate) error: Option<String>,
    /// Overrides the recorded opcode, faults are reported as `REVERT`.
    pub(crate) op: Option<u8>,
    /// Caller of the current call
    pub(crate) caller: Address,
    /// Address of the current call
    pub(crate) contract: Address,
    /// Value of the current call
    pub(crate) value: U256,
    /// Input of the current call
    pub(crate) input: &'a Bytes,
    /// Id of the current call, see `CallStackItem::id`
    pub(crate) call_id: u64,
}

/// Revokes the JS log object's access to the interpreter's stack and memory when dropped.
#[derive(Debug)]
#[must_use]
pub(crate) struct StepScope<'a> {
    state: &'a Shared<StepLogState>,
    _memory: Ref<'static, [u8]>,
}

impl Drop for StepScope<'_> {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.stack.post = None;
        state.memory.post = None;
    }
}

/// Reusable JS call frame object for enter callbacks.
#[derive(Debug)]
pub(crate) struct ReusableCallFrame {
    state: Shared<CallFrameState>,
    object: JsObject,
}

impl ReusableCallFrame {
    pub(crate) fn new(ctx: &mut Context) -> JsResult<Self> {
        let state = Shared::new(CallFrameState::default());
        let object = JsObject::with_object_proto(ctx.intrinsics());

        let get_from = state_fn(ctx, state.clone(), 0, |state, _, ctx| {
            address_to_uint8_array_value(state.borrow().caller, ctx)
        });
        let get_to = state_fn(ctx, state.clone(), 0, |state, _, ctx| {
            address_to_uint8_array_value(state.borrow().contract, ctx)
        });
        let get_value =
            state_fn(ctx, state.clone(), 0, |state, _, _| Ok(to_bigint(state.borrow().value)));
        let get_input = state_fn(ctx, state.clone(), 0, |state, _, ctx| {
            to_uint8_array_value(state.borrow().input.clone(), ctx)
        });
        let get_gas = state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().gas.into()));
        let get_type = state_fn(ctx, state.clone(), 0, |state, _, _| {
            Ok(call_kind_js_string(state.borrow().kind).into())
        });

        object.set(js_string!("getFrom"), get_from, false, ctx)?;
        object.set(js_string!("getTo"), get_to, false, ctx)?;
        object.set(js_string!("getValue"), get_value, false, ctx)?;
        object.set(js_string!("getInput"), get_input, false, ctx)?;
        object.set(js_string!("getGas"), get_gas, false, ctx)?;
        object.set(js_string!("getType"), get_type, false, ctx)?;

        Ok(Self { state, object })
    }

    pub(crate) fn update(&self, frame: CallFrame) {
        let CallFrame { contract, kind, gas } = frame;
        let mut state = self.state.borrow_mut();
        state.caller = contract.caller;
        state.contract = contract.contract;
        state.value = contract.value;
        state.input = contract.input;
        state.gas = gas;
        state.kind = kind;
    }

    pub(crate) fn value(&self) -> JsValue {
        self.object.clone().into()
    }
}

/// Reusable JS frame result object for exit callbacks.
#[derive(Debug)]
pub(crate) struct ReusableFrameResult {
    state: Shared<FrameResultState>,
    object: JsObject,
}

impl ReusableFrameResult {
    pub(crate) fn new(ctx: &mut Context) -> JsResult<Self> {
        let state = Shared::new(FrameResultState::default());
        let object = JsObject::with_object_proto(ctx.intrinsics());

        let get_gas_used =
            state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().gas_used.into()));
        let get_output = state_fn(ctx, state.clone(), 0, |state, _, ctx| {
            to_uint8_array_value(state.borrow().output.clone(), ctx)
        });
        let get_error = state_fn(ctx, state.clone(), 0, |state, _, _| {
            Ok(state
                .borrow()
                .error
                .as_ref()
                .map(|error| JsValue::from(js_string!(error.as_str())))
                .unwrap_or_else(JsValue::undefined))
        });

        object.set(js_string!("getGasUsed"), get_gas_used, false, ctx)?;
        object.set(js_string!("getOutput"), get_output, false, ctx)?;
        object.set(js_string!("getError"), get_error, false, ctx)?;

        Ok(Self { state, object })
    }

    pub(crate) fn update(&self, frame: FrameResult) {
        let FrameResult { gas_used, output, error } = frame;
        let mut state = self.state.borrow_mut();
        state.gas_used = gas_used;
        state.output = output;
        state.error = error;
    }

    pub(crate) fn value(&self) -> JsValue {
        self.object.clone().into()
    }
}

/// Reusable JS database object for step, fault and result callbacks.
///
/// The object is built once, [`Self::enter`] points it at the state and database of the current
/// transaction for the duration of a callback.
#[derive(Debug)]
pub(crate) struct ReusableEvmDb {
    state: Shared<EvmDbState>,
    object: JsObject,
}

impl ReusableEvmDb {
    pub(crate) fn new(ctx: &mut Context) -> JsResult<Self> {
        let state = Shared::new(EvmDbState::default());
        let object = JsObject::with_object_proto(ctx.intrinsics());

        // Note: the arguments are converted before the state is borrowed mutably, since
        // converting a value may run JS code.
        let exists = state_fn(ctx, state.clone(), 1, |state, args, ctx| {
            let address = address_from_value(args.get_or_undefined(0).clone(), ctx)?;
            let acc = state.borrow_mut().read_basic(address)?;
            Ok(JsValue::from(acc.is_some()))
        });
        let get_balance = state_fn(ctx, state.clone(), 1, |state, args, ctx| {
            let address = address_from_value(args.get_or_undefined(0).clone(), ctx)?;
            let acc = state.borrow_mut().read_basic(address)?;
            Ok(to_bigint(acc.map(|acc| acc.balance).unwrap_or_default()))
        });
        let get_nonce = state_fn(ctx, state.clone(), 1, |state, args, ctx| {
            let address = address_from_value(args.get_or_undefined(0).clone(), ctx)?;
            let acc = state.borrow_mut().read_basic(address)?;
            Ok(JsValue::from(acc.map(|acc| acc.nonce).unwrap_or_default()))
        });
        let get_code = state_fn(ctx, state.clone(), 1, |state, args, ctx| {
            let address = address_from_value(args.get_or_undefined(0).clone(), ctx)?;
            let code = state.borrow_mut().read_code(address)?;
            to_uint8_array_value(code, ctx)
        });
        let get_state = state_fn(ctx, state.clone(), 2, |state, args, ctx| {
            let address = address_from_value(args.get_or_undefined(0).clone(), ctx)?;
            let slot = b256_from_value(args.get_or_undefined(1).clone(), ctx)?;
            let value = state.borrow_mut().read_state(address, slot)?;
            to_uint8_array_value(B256::from(value), ctx)
        });

        object.set(js_string!("getBalance"), get_balance, false, ctx)?;
        object.set(js_string!("getNonce"), get_nonce, false, ctx)?;
        object.set(js_string!("getCode"), get_code, false, ctx)?;
        object.set(js_string!("getState"), get_state, false, ctx)?;
        object.set(js_string!("exists"), exists, false, ctx)?;

        Ok(Self { state, object })
    }

    /// Gives the JS object access to the given state and database until the returned guard is
    /// dropped.
    pub(crate) fn enter<'a>(&'a self, state: &EvmState, db: &mut dyn ErasedDb) -> DbScope<'a> {
        // SAFETY: boa requires 'static values, the guard removes the references from the shared
        // state when it is dropped and JS code only runs while the guard is alive.
        let state: &'static EvmState = unsafe { core::mem::transmute(state) };
        let db: &'static mut dyn ErasedDb = unsafe {
            core::mem::transmute::<&mut (dyn ErasedDb + '_), &'static mut (dyn ErasedDb + 'static)>(
                db,
            )
        };
        let mut shared = self.state.borrow_mut();
        shared.state = Some(state);
        shared.db = Some(db);
        drop(shared);
        DbScope { state: &self.state }
    }

    pub(crate) fn value(&self) -> JsValue {
        self.object.clone().into()
    }
}

/// Revokes the JS database object's access to the state and database when dropped.
#[derive(Debug)]
#[must_use]
pub(crate) struct DbScope<'a> {
    state: &'a Shared<EvmDbState>,
}

impl Drop for DbScope<'_> {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.state = None;
        state.db = None;
    }
}

/// Object safe view of a [`Database`] with the error type erased.
pub(crate) trait ErasedDb {
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, String>;
    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, String>;
    fn storage(&mut self, address: Address, index: U256) -> Result<U256, String>;
}

impl<DB: Database> ErasedDb for DB {
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, String> {
        Database::basic(self, address).map_err(|err| err.to_string())
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, String> {
        Database::code_by_hash(self, code_hash).map_err(|err| err.to_string())
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, String> {
        Database::storage(self, address, index).map_err(|err| err.to_string())
    }
}

/// The state and database the JS `db` object currently has access to.
#[derive(Default)]
struct EvmDbState {
    state: Option<&'static EvmState>,
    db: Option<&'static mut dyn ErasedDb>,
}

impl EvmDbState {
    fn active(&mut self) -> JsResult<(&EvmState, &mut dyn ErasedDb)> {
        match (self.state, self.db.as_deref_mut()) {
            (Some(state), Some(db)) => Ok((state, db)),
            _ => Err(type_error("tracer accessed db outside of a callback".to_string())),
        }
    }

    fn read_basic(&mut self, address: Address) -> JsResult<Option<AccountInfo>> {
        let (state, db) = self.active()?;
        if let Some(acc) = state.get(&address) {
            return Ok(Some(acc.info.clone()));
        }
        db.basic(address).map_err(|err| {
            JsError::from_native(
                JsNativeError::error().with_message(format!(
                    "Failed to read address {address:?} from database: {err}"
                )),
            )
        })
    }

    fn read_code(&mut self, address: Address) -> JsResult<Bytes> {
        let acc = self.read_basic(address)?;
        let code_hash = acc.as_ref().map(|acc| acc.code_hash).unwrap_or(KECCAK_EMPTY);
        if code_hash == KECCAK_EMPTY {
            return Ok(Bytes::new());
        }
        if let Some(bytecode) = acc.and_then(|acc| acc.code) {
            return Ok(bytecode.original_bytes());
        }
        let (_, db) = self.active()?;
        db.code_by_hash(code_hash).map(|code| code.original_bytes()).map_err(|err| {
            JsError::from_native(JsNativeError::error().with_message(format!(
                "Failed to read code hash {code_hash:?} from database: {err}"
            )))
        })
    }

    fn read_state(&mut self, address: Address, slot: B256) -> JsResult<U256> {
        let (_, db) = self.active()?;
        db.storage(address, slot.into()).map_err(|err| {
            JsError::from_native(JsNativeError::error().with_message(format!(
                "Failed to read state for {address:?} at {slot:?} from database: {err}"
            )))
        })
    }
}

impl core::fmt::Debug for EvmDbState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EvmDbState")
            .field("state", &self.state.is_some())
            .field("db", &self.db.is_some())
            .finish()
    }
}

#[derive(Debug)]
struct StepLogState {
    op: u8,
    pc: u64,
    gas_remaining: u64,
    cost: u64,
    depth: u64,
    refund: u64,
    error: Option<String>,
    contract: Contract,
    /// Id of the call `contract` belongs to.
    call_id: u64,
    stack: StackView,
    memory: MemoryView,
    op_names: OpcodeNames,
}

impl StepLogState {
    fn new() -> Self {
        Self {
            op: 0,
            pc: 0,
            gas_remaining: 0,
            cost: 0,
            depth: 0,
            refund: 0,
            error: None,
            contract: Contract::default(),
            call_id: 0,
            stack: StackView::default(),
            memory: MemoryView::default(),
            op_names: OpcodeNames::new(),
        }
    }
}

/// JS strings of all opcode names, so `log.op.toString()` does not allocate.
#[derive(Debug)]
struct OpcodeNames(Vec<JsString>);

impl OpcodeNames {
    fn new() -> Self {
        Self(
            (0..=u8::MAX)
                .map(|op| match OpCode::new(op) {
                    Some(op) => JsString::from(op.as_str()),
                    None => JsString::from(format!("opcode {op:x} not defined").as_str()),
                })
                .collect(),
        )
    }

    fn get(&self, op: u8) -> JsString {
        self.0[op as usize].clone()
    }
}

/// Pre-execution view of the stack, reconstructed from the post-execution stack and the items the
/// opcode consumed.
///
/// An opcode only touches its top `inputs` items, so the pre-execution stack is the post-execution
/// stack below those items followed by the saved inputs. This holds for halted opcodes too, which
/// pop at most `inputs` items.
#[derive(Debug, Default)]
struct StackView {
    /// Pre-execution length.
    len: usize,
    /// The top `saved.len()` pre-execution items, bottom to top.
    saved: Vec<U256>,
    /// Post-execution stack, only set while a callback runs.
    post: Option<&'static [U256]>,
}

impl StackView {
    fn record(&mut self, op: u8, stack: &[U256]) {
        self.len = stack.len();
        let inputs = OpCode::new(op).map_or(0, |op| op.inputs() as usize).min(stack.len());
        self.saved.clear();
        self.saved.extend_from_slice(&stack[stack.len() - inputs..]);
    }

    /// Returns the pre-execution length, or 0 outside of a callback.
    fn len(&self) -> usize {
        if self.post.is_some() {
            self.len
        } else {
            0
        }
    }

    /// Returns the item `idx` positions from the top of the pre-execution stack.
    fn peek(&self, idx: usize) -> JsResult<U256> {
        let Some(post) = self.post else {
            return Err(type_error("tracer accessed stack outside of a callback".to_string()));
        };
        let saved = self.saved.len();
        let item = if idx < saved {
            Some(self.saved[saved - 1 - idx])
        } else {
            (idx < self.len).then(|| post.get(self.len - 1 - idx).copied()).flatten()
        };
        item.ok_or_else(|| {
            type_error(format!(
                "tracer accessed out of bound stack: size {}, index {idx}",
                self.len
            ))
        })
    }
}

/// Pre-execution view of memory, reconstructed from the post-execution memory, the pre-execution
/// length and the bytes the opcode overwrote.
///
/// Opcodes only ever expand memory or overwrite a single range, so restoring that range and
/// truncating to the pre-execution length yields the pre-execution memory. Return data of calls is
/// written when the child frame returns, i.e. after `step_end` of the call opcode.
#[derive(Debug, Default)]
struct MemoryView {
    /// Pre-execution length.
    len: usize,
    /// Offset of `patch` in memory.
    patch_offset: usize,
    /// Pre-execution bytes of the range the opcode overwrites.
    patch: Vec<u8>,
    /// Post-execution memory, only set while a callback runs.
    post: Option<&'static [u8]>,
}

impl MemoryView {
    fn record(&mut self, op: u8, stack: &[U256], memory: &[u8]) {
        self.len = memory.len();
        self.patch.clear();
        self.patch_offset = 0;
        if let Some((offset, size)) = memory_write_range(op, stack) {
            if offset < memory.len() && size > 0 {
                let end = offset.saturating_add(size).min(memory.len());
                self.patch_offset = offset;
                self.patch.extend_from_slice(&memory[offset..end]);
            }
        }
    }

    /// Returns the pre-execution length, or 0 outside of a callback.
    fn len(&self) -> usize {
        if self.post.is_some() {
            self.len
        } else {
            0
        }
    }

    /// Returns the pre-execution bytes of the given range, which must be within
    /// [`Self::len`].
    fn bytes(&self, range: Range<usize>) -> JsResult<AlignedVec<u8>> {
        let Some(post) = self.post else {
            return Err(type_error("tracer accessed memory outside of a callback".to_string()));
        };
        let patch = self.patch_offset..self.patch_offset + self.patch.len();
        Ok(AlignedVec::from_iter(
            0,
            range.map(|i| {
                if patch.contains(&i) {
                    self.patch[i - self.patch_offset]
                } else {
                    post.get(i).copied().unwrap_or_default()
                }
            }),
        ))
    }
}

/// Returns the memory range `(offset, size)` the opcode overwrites, based on its pre-execution
/// stack.
///
/// Returns `None` for opcodes that don't write to memory, or if the operands are out of range, in
/// which case the opcode fails without writing.
fn memory_write_range(op: u8, stack: &[U256]) -> Option<(usize, usize)> {
    let peek = |n: usize| stack.len().checked_sub(n + 1).map(|i| stack[i]);
    let (offset, size) = match op {
        opcode::MSTORE => (peek(0)?, U256::from(32)),
        opcode::MSTORE8 => (peek(0)?, U256::from(1)),
        opcode::MCOPY | opcode::CALLDATACOPY | opcode::CODECOPY | opcode::RETURNDATACOPY => {
            (peek(0)?, peek(2)?)
        }
        opcode::EXTCODECOPY => (peek(1)?, peek(3)?),
        _ => return None,
    };
    Some((usize::try_from(offset).ok()?, usize::try_from(size).ok()?))
}

#[derive(Debug, Default)]
struct CallFrameState {
    caller: Address,
    contract: Address,
    value: U256,
    input: Bytes,
    gas: u64,
    kind: CallKind,
}

#[derive(Debug, Default)]
struct FrameResultState {
    gas_used: u64,
    output: Bytes,
    error: Option<String>,
}

fn build_step_op_object(state: Shared<StepLogState>, ctx: &mut Context) -> JsResult<JsObject> {
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    let to_number = state_fn(ctx, state.clone(), 0, |state, _, _| Ok(state.borrow().op.into()));
    let is_push = state_fn(ctx, state.clone(), 0, |state, _, _| {
        Ok(JsValue::from((PUSH0..=PUSH32).contains(&state.borrow().op)))
    });
    let to_string = state_fn(ctx, state, 0, |state, _, _| {
        let state = state.borrow();
        Ok(state.op_names.get(state.op).into())
    });

    obj.set(js_string!("toNumber"), to_number, false, ctx)?;
    obj.set(js_string!("toString"), to_string, false, ctx)?;
    obj.set(js_string!("isPush"), is_push, false, ctx)?;
    Ok(obj)
}

fn build_step_stack_object(state: Shared<StepLogState>, ctx: &mut Context) -> JsResult<JsObject> {
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    let length = state_fn(ctx, state.clone(), 0, |state, _, _| {
        Ok(JsValue::from(state.borrow().stack.len()))
    });
    let peek = state_fn(ctx, state, 1, |state, args, ctx| {
        let len = state.borrow().stack.len();
        let idx = parse_stack_index(args.get_or_undefined(0), len, ctx)?;
        Ok(to_bigint(state.borrow().stack.peek(idx)?))
    });

    obj.set(js_string!("length"), length, false, ctx)?;
    obj.set(js_string!("peek"), peek, false, ctx)?;
    Ok(obj)
}

fn build_step_memory_object(state: Shared<StepLogState>, ctx: &mut Context) -> JsResult<JsObject> {
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    let length = state_fn(ctx, state.clone(), 0, |state, _, _| {
        Ok(JsValue::from(state.borrow().memory.len() as u64))
    });
    let slice = state_fn(ctx, state.clone(), 2, |state, args, ctx| {
        let len = state.borrow().memory.len();
        let start = parse_memory_index(args.get_or_undefined(0), "start", len, ctx)?;
        let end = parse_memory_index(args.get_or_undefined(1), "end", len, ctx)?;
        if end < start || end > len {
            return Err(memory_out_of_bounds_error(len, start, end.saturating_sub(start)));
        }
        let bytes = state.borrow().memory.bytes(start..end)?;
        uint8_array_from_block(bytes, ctx)
    });
    let get_uint = state_fn(ctx, state, 1, |state, args, ctx| {
        let len = state.borrow().memory.len();
        let offset = parse_memory_index(args.get_or_undefined(0), "offset", len, ctx)?;
        let Some(end) = offset.checked_add(32) else {
            return Err(memory_out_of_bounds_error(len, offset, 32));
        };
        if end > len {
            return Err(memory_out_of_bounds_error(len, offset, 32));
        }
        let bytes = state.borrow().memory.bytes(offset..end)?;
        uint8_array_from_block(bytes, ctx)
    });

    obj.set(js_string!("slice"), slice, false, ctx)?;
    obj.set(js_string!("getUint"), get_uint, false, ctx)?;
    obj.set(js_string!("length"), length, false, ctx)?;
    Ok(obj)
}

fn build_step_contract_object(
    state: Shared<StepLogState>,
    ctx: &mut Context,
) -> JsResult<JsObject> {
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    let get_caller = state_fn(ctx, state.clone(), 0, |state, _, ctx| {
        address_to_uint8_array_value(state.borrow().contract.caller, ctx)
    });
    let get_address = state_fn(ctx, state.clone(), 0, |state, _, ctx| {
        address_to_uint8_array_value(state.borrow().contract.contract, ctx)
    });
    let get_value =
        state_fn(ctx, state.clone(), 0, |state, _, _| Ok(to_bigint(state.borrow().contract.value)));
    let get_input = state_fn(ctx, state, 0, |state, _, ctx| {
        to_uint8_array_value(state.borrow().contract.input.clone(), ctx)
    });

    obj.set(js_string!("getCaller"), get_caller, false, ctx)?;
    obj.set(js_string!("getAddress"), get_address, false, ctx)?;
    obj.set(js_string!("getValue"), get_value, false, ctx)?;
    obj.set(js_string!("getInput"), get_input, false, ctx)?;
    Ok(obj)
}

fn parse_stack_index(value: &JsValue, len: usize, ctx: &mut Context) -> JsResult<usize> {
    let index = value.to_numeric_number(ctx)?;
    if !index.is_finite() || index < 0. || index > usize::MAX as f64 {
        return Err(type_error(format!(
            "tracer accessed out of bound stack: size {len}, index {index}"
        )));
    }
    Ok(index as usize)
}

fn invalid_memory_index_error(name: &str, index: impl core::fmt::Display) -> JsError {
    type_error(format!("invalid memory {name}: {index}"))
}

fn memory_out_of_bounds_error(len: usize, offset: usize, size: usize) -> JsError {
    type_error(format!(
        "tracer accessed out of bound memory: available {len}, offset {offset}, size {size}"
    ))
}

fn parse_memory_index(
    value: &JsValue,
    name: &str,
    len: usize,
    ctx: &mut Context,
) -> JsResult<usize> {
    if value.is_undefined() {
        return Err(invalid_memory_index_error(name, "undefined"));
    }
    if let Some(index) = value.as_number() {
        if !index.is_finite() || index < 0. {
            return Err(invalid_memory_index_error(name, index));
        }
    }
    let index = if let Some(index) = value.as_bigint() {
        // Boa's `ToIndex` rejects BigInt, but stack-derived tracer values are BigInt.
        let index = index.to_string();
        index.parse::<usize>().map_err(|_| invalid_memory_index_error(name, &index))?
    } else {
        let index = value.to_index(ctx)?;
        usize::try_from(index).map_err(|_| invalid_memory_index_error(name, index))?
    };
    if index > len {
        return Err(invalid_memory_index_error(name, index));
    }
    Ok(index)
}

/// Represents the contract object
#[derive(Clone, Debug, Default)]
pub(crate) struct Contract {
    pub(crate) caller: Address,
    pub(crate) contract: Address,
    pub(crate) value: U256,
    pub(crate) input: Bytes,
}

/// Represents the call frame object for exit functions
pub(crate) struct FrameResult {
    pub(crate) gas_used: u64,
    pub(crate) output: Bytes,
    pub(crate) error: Option<String>,
}

/// Represents the call frame object for enter functions
pub(crate) struct CallFrame {
    pub(crate) contract: Contract,
    pub(crate) kind: CallKind,
    pub(crate) gas: u64,
}

/// The `ctx` object that represents the context in which the transaction is executed.
pub(crate) struct JsEvmContext {
    /// String, one of the two values CALL and CREATE
    pub(crate) r#type: String,
    /// Sender of the transaction
    pub(crate) from: Address,
    /// Target of the transaction
    pub(crate) to: Option<Address>,
    pub(crate) input: Bytes,
    /// Gas limit
    pub(crate) gas: u64,
    /// Number, amount of gas used in executing the transaction (excludes txdata costs)
    pub(crate) gas_used: u64,
    /// Number, gas price configured in the transaction being executed
    pub(crate) gas_price: u64,
    /// Number, intrinsic gas for the transaction being executed
    pub(crate) intrinsic_gas: u64,
    /// big.int Amount to be transferred in wei
    pub(crate) value: U256,
    /// Number, block number
    pub(crate) block: u64,
    /// Address, miner of the block
    pub(crate) coinbase: Address,
    pub(crate) output: Bytes,
    /// Number, block timestamp
    pub(crate) time: String,
    pub(crate) transaction_ctx: TransactionContext,
    /// returns information about the error if one occurred, otherwise returns undefined
    pub(crate) error: Option<String>,
}

impl JsEvmContext {
    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let Self {
            r#type,
            from,
            to,
            input,
            gas,
            gas_used,
            gas_price,
            intrinsic_gas,
            value,
            block,
            coinbase,
            output,
            time,
            transaction_ctx,
            error,
        } = self;
        let obj = JsObject::with_object_proto(ctx.intrinsics());

        // add properties

        obj.set(js_string!("type"), js_string!(r#type), false, ctx)?;
        obj.set(js_string!("from"), address_to_uint8_array(from, ctx)?, false, ctx)?;
        if let Some(to) = to {
            obj.set(js_string!("to"), address_to_uint8_array(to, ctx)?, false, ctx)?;
        } else {
            obj.set(js_string!("to"), JsValue::null(), false, ctx)?;
        }

        obj.set(js_string!("input"), to_uint8_array(input, ctx)?, false, ctx)?;
        obj.set(js_string!("gas"), gas, false, ctx)?;
        obj.set(js_string!("gasUsed"), gas_used, false, ctx)?;
        obj.set(js_string!("gasPrice"), gas_price, false, ctx)?;
        obj.set(js_string!("intrinsicGas"), intrinsic_gas, false, ctx)?;
        obj.set(js_string!("value"), to_bigint(value), false, ctx)?;
        obj.set(js_string!("block"), block, false, ctx)?;
        obj.set(js_string!("coinbase"), address_to_uint8_array(coinbase, ctx)?, false, ctx)?;
        obj.set(js_string!("output"), to_uint8_array(output, ctx)?, false, ctx)?;
        obj.set(js_string!("time"), js_string!(time), false, ctx)?;
        if let Some(block_hash) = transaction_ctx.block_hash {
            obj.set(js_string!("blockHash"), to_uint8_array(block_hash, ctx)?, false, ctx)?;
        }
        if let Some(tx_index) = transaction_ctx.tx_index {
            obj.set(js_string!("txIndex"), tx_index as u64, false, ctx)?;
        }
        if let Some(tx_hash) = transaction_ctx.tx_hash {
            obj.set(js_string!("txHash"), to_uint8_array(tx_hash, ctx)?, false, ctx)?;
        }
        if let Some(error) = error {
            obj.set(js_string!("error"), js_string!(error), false, ctx)?;
        }

        Ok(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing::js::builtins::{
        bytes_from_value, json_stringify, register_builtins, to_serde_value,
    };
    use alloc::vec;
    use boa_engine::{object::builtins::JsUint8Array, Source};
    use revm::{database::CacheDB, database_interface::EmptyDB};

    /// Records and enters a step with the given pre- and post-execution stack.
    fn enter_step<'a>(
        log: &'a ReusableStepLog,
        op: u8,
        stack: &[U256],
        memory: &'a RefCell<Vec<u8>>,
        contract: &'a Contract,
    ) -> StepScope<'a> {
        log.record_pre_execution(PreStep {
            pc: 0,
            op,
            gas_remaining: 0,
            refund: 0,
            stack,
            memory: &memory.borrow(),
        });
        log.enter(
            stack,
            Ref::map(memory.borrow(), Vec::as_slice),
            StepInfo {
                cost: 0,
                depth: 0,
                error: None,
                op: None,
                caller: contract.caller,
                contract: contract.contract,
                value: contract.value,
                input: &contract.input,
                call_id: 1,
            },
        )
    }

    #[test]
    fn test_contract() {
        let mut ctx = Context::default();
        let contract = Contract {
            caller: Address::ZERO,
            contract: Address::ZERO,
            value: U256::from(1337u64),
            input: vec![0x01, 0x02, 0x03].into(),
        };
        register_builtins(&mut ctx).unwrap();

        let memory = RefCell::new(Vec::new());
        let reusable_step = ReusableStepLog::new(&mut ctx).unwrap();
        let _scope = enter_step(&reusable_step, 0, &[], &memory, &contract);

        let s = "({
                caller: function(log) { return log.contract.getCaller(); },
                value: function(log) { return log.contract.getValue(); },
                address: function(log) { return log.contract.getAddress(); },
                input: function(log) { return log.contract.getInput(); }
        })";

        let log_arg = reusable_step.value();
        let eval_obj = ctx.eval(Source::from_bytes(s)).unwrap();
        let call = eval_obj.as_object().unwrap().get(js_string!("caller"), &mut ctx).unwrap();
        let res = call
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), core::slice::from_ref(&log_arg), &mut ctx)
            .unwrap();
        assert!(res.is_object());
        let obj = res.as_object().unwrap();
        let array_buf = JsUint8Array::from_object(obj.clone());
        assert!(array_buf.is_ok());

        let get_address =
            eval_obj.as_object().unwrap().get(js_string!("address"), &mut ctx).unwrap();
        let res = get_address
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), core::slice::from_ref(&log_arg), &mut ctx)
            .unwrap();
        assert!(res.is_object());

        let buf = bytes_from_value(res, &mut ctx).unwrap();
        assert_eq!(buf, contract.contract.as_slice());

        let call = eval_obj.as_object().unwrap().get(js_string!("value"), &mut ctx).unwrap();
        let res = call
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), core::slice::from_ref(&log_arg), &mut ctx)
            .unwrap();
        assert_eq!(
            res.to_string(&mut ctx).unwrap().to_std_string().unwrap(),
            contract.value.to_string()
        );

        let call = eval_obj.as_object().unwrap().get(js_string!("input"), &mut ctx).unwrap();
        let res =
            call.as_callable().unwrap().call(&JsValue::undefined(), &[log_arg], &mut ctx).unwrap();

        let buf = bytes_from_value(res, &mut ctx).unwrap();
        assert_eq!(buf, contract.input);
    }

    #[test]
    fn test_evm_db_gc() {
        let mut context = Context::default();

        let result = context
            .eval(Source::from_bytes(
                "(
                    function(db, addr) {return db.exists(addr) }
            )
        "
                .to_string()
                .as_bytes(),
            ))
            .unwrap();
        assert!(result.is_callable());

        let f = result.as_callable().unwrap();

        let mut db = CacheDB::new(EmptyDB::new());
        let state = EvmState::default();
        let reusable_db = ReusableEvmDb::new(&mut context).unwrap();
        {
            let guard = reusable_db.enter(&state, &mut db);
            let addr = Address::default();
            let addr = JsValue::from(js_string!(addr.to_string()));
            let db = reusable_db.value();
            let res = f.call(&result, &[db.clone(), addr.clone()], &mut context).unwrap();
            assert!(!res.as_boolean().unwrap());

            // drop the guard which revokes the db access
            drop(guard);
            let res = f.call(&result, &[db.clone(), addr.clone()], &mut context);
            assert!(res.is_err());
        }
        let addr = Address::default();
        db.insert_account_info(addr, Default::default());

        {
            let guard = reusable_db.enter(&state, &mut db);
            let addr = JsValue::from(js_string!(addr.to_string()));
            let db = reusable_db.value();
            let res = f.call(&result, &[db.clone(), addr.clone()], &mut context).unwrap();

            // account exists
            assert!(res.as_boolean().unwrap());

            // drop the guard which revokes the db access
            drop(guard);
            let res = f.call(&result, &[db.clone(), addr.clone()], &mut context);
            assert!(res.is_err());
        }
    }

    #[test]
    fn test_evm_db_gc_captures() {
        let mut context = Context::default();

        let res = context
            .eval(Source::from_bytes(
                r"({
                 setup: function(db) {this.db = db;},
                 result: function(addr) {return this.db.exists(addr) }
            })
        "
                .to_string()
                .as_bytes(),
            ))
            .unwrap();

        let obj = res.as_object().unwrap();

        let result_fn = obj.get(js_string!("result"), &mut context).unwrap().as_object().unwrap();
        let setup_fn = obj.get(js_string!("setup"), &mut context).unwrap().as_object().unwrap();

        let mut db = CacheDB::new(EmptyDB::new());
        let state = EvmState::default();
        {
            let reusable_db = ReusableEvmDb::new(&mut context).unwrap();
            let guard = reusable_db.enter(&state, &mut db);
            let _res =
                setup_fn.call(&(obj.clone().into()), &[reusable_db.value()], &mut context).unwrap();
            assert!(obj.get(js_string!("db"), &mut context).unwrap().is_object());

            let addr = Address::default();
            let addr = JsValue::from(js_string!(addr.to_string()));
            let res = result_fn
                .call(&(obj.clone().into()), core::slice::from_ref(&addr), &mut context)
                .unwrap();
            assert!(!res.as_boolean().unwrap());

            // drop the guard which revokes the db access
            drop(guard);
            let res = result_fn.call(&(obj.clone().into()), &[addr], &mut context);
            assert!(res.is_err());
        }
    }

    #[test]
    fn test_big_int() {
        let mut context = Context::default();
        register_builtins(&mut context).unwrap();

        let eval = context
            .eval(Source::from_bytes(
                r#"({data: [], fault: function(log) {}, step: function(log) { this.data.push({ value: log.stack.peek(2) }) }, result: function() { return this.data; }})"#
                .to_string()
                .as_bytes(),
            ))
            .unwrap();

        let obj = eval.as_object().unwrap();

        let result_fn = obj.get(js_string!("result"), &mut context).unwrap().as_object().unwrap();
        let step_fn = obj.get(js_string!("step"), &mut context).unwrap().as_object().unwrap();

        let stack = [U256::from(35000); 3];
        let memory = RefCell::new(Vec::new());
        let contract = Contract::default();
        let reusable_step = ReusableStepLog::new(&mut context).unwrap();
        let scope = enter_step(&reusable_step, 0, &stack, &memory, &contract);

        let _ = step_fn.call(&eval, &[reusable_step.value()], &mut context).unwrap();
        drop(scope);

        let res = result_fn.call(&eval, &[], &mut context).unwrap();
        let val = json_stringify(res.clone(), &mut context).unwrap().to_std_string().unwrap();
        assert_eq!(val, r#"[{"value":"35000"}]"#);

        let val = to_serde_value(res, &mut context).unwrap();
        assert!(val.is_array());
        let s = val.to_string();
        assert_eq!(s, r#"[{"value":"35000"}]"#);
    }

    #[test]
    fn test_object_functions() {
        let mut context = Context::default();
        register_builtins(&mut context).unwrap();

        let eval = context
            .eval(Source::from_bytes(
                r#"(
    {
        retVal: [],
        callStack: [],
        byte2Hex: function (byte) {
            if (byte < 0x10) return "0" + byte.toString(16);
            return byte.toString(16);
        },
        array2Hex: function (arr) {
            var retVal = "";
            for (var i = 0; i < arr.length; i++) retVal += this.byte2Hex(arr[i]);
            return retVal;
        },
        getAddr: function (log) {
            return this.array2Hex(log.contract.getAddress());
        },
        step: function (log, db) {
            var opcode = log.op.toNumber();
            if (opcode == 0x54) {
                this.retVal.push(this.getAddr(log) + ":" + log.stack.peek(0).toString(16));
            }
            if (opcode == 0x55)
                this.retVal.push(
                    this.getAddr(log) +
                        ":" +
                        log.stack.peek(0).toString(16) +
                        ";" +
                        log.stack.peek(1).toString(16)
                );
        },
        fault: function (log, db) {
            this.retVal.push("FAULT: ");
        },
        result: function (ctx, db) {
            return this.retVal;
        },
   }
)"#
                .to_string()
                .as_bytes(),
            ))
            .unwrap();

        let obj = eval.as_object().unwrap();

        let result_fn = obj.get(js_string!("result"), &mut context).unwrap().as_object().unwrap();
        let step_fn = obj.get(js_string!("step"), &mut context).unwrap().as_object().unwrap();

        let stack = [U256::from(35000); 3];
        let memory = RefCell::new(Vec::new());
        let contract = Contract::default();
        let reusable_step = ReusableStepLog::new(&mut context).unwrap();
        let scope = enter_step(&reusable_step, 85, &stack, &memory, &contract);

        let _ = step_fn.call(&eval, &[reusable_step.value()], &mut context).unwrap();
        drop(scope);

        let res = result_fn.call(&eval, &[], &mut context).unwrap();
        let val = json_stringify(res.clone(), &mut context).unwrap().to_std_string().unwrap();
        assert_eq!(val, r#"["0000000000000000000000000000000000000000:88b8;88b8"]"#);
    }

    #[test]
    fn test_stack_view_reconstructs_pre_execution_stack() {
        // SWAP1 touches the top two items, DUP1 one item, ADD two items
        let pre = [U256::from(1), U256::from(2), U256::from(3)];
        for (op, post) in [
            (opcode::SWAP1, vec![U256::from(1), U256::from(3), U256::from(2)]),
            (opcode::DUP1, vec![U256::from(1), U256::from(2), U256::from(3), U256::from(3)]),
            (opcode::ADD, vec![U256::from(1), U256::from(5)]),
            (opcode::POP, vec![U256::from(1), U256::from(2)]),
            // halted before pushing, e.g. out of gas after popping the inputs
            (opcode::MSTORE, vec![U256::from(1)]),
            (opcode::STOP, pre.to_vec()),
        ] {
            let mut view = StackView::default();
            view.record(op, &pre);
            let post: &'static [U256] = post.leak();
            view.post = Some(post);
            assert_eq!(view.len(), 3);
            for (idx, expected) in pre.iter().rev().enumerate() {
                assert_eq!(view.peek(idx).unwrap(), *expected, "op {op:#x} idx {idx}");
            }
            assert!(view.peek(3).is_err());
        }
    }

    #[test]
    fn test_memory_view_reconstructs_pre_execution_memory() {
        let pre: Vec<u8> = (0..64).collect();
        // MSTORE at offset 16, expanding memory to 96 bytes
        let mut post = pre.clone();
        post[16..48].copy_from_slice(&[0xff; 32]);
        post.resize(96, 0);
        let stack = [U256::from(16)];

        let mut view = MemoryView::default();
        view.record(opcode::MSTORE, &stack, &pre);
        assert_eq!(view.patch_offset, 16);
        assert_eq!(view.patch, pre[16..48]);
        view.post = Some(post.leak());

        assert_eq!(view.len(), 64);
        assert_eq!(view.bytes(0..64).unwrap().as_slice(), pre.as_slice());
        assert_eq!(view.bytes(10..20).unwrap().as_slice(), &pre[10..20]);

        // writes beyond the pre-execution length only expand memory
        let mut view = MemoryView::default();
        view.record(opcode::MSTORE, &[U256::from(64)], &pre);
        assert!(view.patch.is_empty());

        // MCOPY: dst = 0, src = 32, len = 8, operands are popped top first
        let mut view = MemoryView::default();
        view.record(opcode::MCOPY, &[U256::from(8), U256::from(32), U256::ZERO], &pre);
        assert_eq!(view.patch_offset, 0);
        assert_eq!(view.patch, pre[..8]);

        // EXTCODECOPY: address, dst = 4, src = 0, len = 2
        let mut view = MemoryView::default();
        view.record(
            opcode::EXTCODECOPY,
            &[U256::from(2), U256::ZERO, U256::from(4), U256::from(1)],
            &pre,
        );
        assert_eq!(view.patch_offset, 4);
        assert_eq!(view.patch, pre[4..6]);

        // out of range operands fail without writing
        let mut view = MemoryView::default();
        view.record(opcode::MSTORE, &[U256::MAX], &pre);
        assert!(view.patch.is_empty());
        let mut view = MemoryView::default();
        view.record(opcode::MSTORE, &[], &pre);
        assert!(view.patch.is_empty());
    }
}
