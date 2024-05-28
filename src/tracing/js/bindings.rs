//! Type bindings for js tracing inspector

use crate::tracing::{
    js::{
        builtins::{
            address_to_buf, bytes_to_address, bytes_to_hash, from_buf, to_bigint, to_buf,
            to_buf_value,
        },
        TransactionContext,
    },
    types::CallKind,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use boa_engine::{
    js_string,
    native_function::NativeFunction,
    object::{builtins::JsArrayBuffer, FunctionObjectBuilder},
    Context, JsArgs, JsError, JsNativeError, JsObject, JsResult, JsValue,
};
use boa_gc::{empty_trace, Finalize, Trace};
use revm::{
    interpreter::{
        opcode::{PUSH0, PUSH32},
        OpCode, SharedMemory, Stack,
    },
    primitives::{AccountInfo, Bytecode, EvmState, KECCAK_EMPTY},
    DatabaseRef,
};
use std::{cell::RefCell, rc::Rc};

/// A macro that creates a native function that returns via [JsValue::from]
macro_rules! js_value_getter {
    ($value:ident, $ctx:ident) => {
        FunctionObjectBuilder::new(
            $ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, _ctx| Ok(JsValue::from($value))),
        )
        .length(0)
        .build()
    };
}

/// A macro that creates a native function that returns a captured JsValue
macro_rules! js_value_capture_getter {
    ($value:ident, $ctx:ident) => {
        FunctionObjectBuilder::new(
            $ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, _args, input, _ctx| Ok(JsValue::from(input.clone())),
                $value,
            ),
        )
        .length(0)
        .build()
    };
}

/// A wrapper for a value that can be garbage collected, but will not give access to the value if
/// it has been dropped via its guard.
///
/// This is used to allow the JS tracer functions to access values at a certain point during
/// inspection by ref without having to clone them and capture them in the js object.
///
/// JS tracer functions get access to evm internals via objects or function arguments, for example
/// `function step(log,evm)` where log has an object `stack` that has a function `peek(number)` that
/// returns a value from the stack.
///
/// These functions could get garbage collected, however the data accessed by the function is
/// supposed to be ephemeral and only valid for the duration of the function call.
///
/// This type supports garbage collection of (rust) references and prevents access to the value if
/// it has been dropped.
#[derive(Clone, Debug)]
struct GuardedNullableGc<Val: 'static> {
    /// The lifetime is a lie to make it possible to use a reference in boa which requires 'static
    inner: Rc<RefCell<Option<Guarded<'static, Val>>>>,
}

impl<Val: 'static> GuardedNullableGc<Val> {
    /// Creates a garbage collectible value to the given reference.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the guard is dropped before the value is dropped.
    fn new_ref(val: &Val) -> (Self, GcGuard<'_, Val>) {
        Self::new(Guarded::Ref(val))
    }

    /// Creates a garbage collectible value to the given reference.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the guard is dropped before the value is dropped.
    fn new_owned<'a>(val: Val) -> (Self, GcGuard<'a, Val>) {
        Self::new(Guarded::Owned(val))
    }

    fn new(val: Guarded<'_, Val>) -> (Self, GcGuard<'_, Val>) {
        let inner = Rc::new(RefCell::new(Some(val)));
        let guard = GcGuard { inner: Rc::clone(&inner) };

        // SAFETY: guard enforces that the value is removed from the refcell before it is dropped.
        #[allow(clippy::missing_transmute_annotations)]
        let this = Self { inner: unsafe { std::mem::transmute(inner) } };

        (this, guard)
    }

    /// Executes the given closure with a reference to the inner value if it is still present.
    fn with_inner<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&Val) -> R,
    {
        self.inner.borrow().as_ref().map(|guard| f(guard.as_ref()))
    }
}

impl<Val: 'static> Finalize for GuardedNullableGc<Val> {}

unsafe impl<Val: 'static> Trace for GuardedNullableGc<Val> {
    empty_trace!();
}

/// A value that is either a reference or an owned value.
#[derive(Debug)]
enum Guarded<'a, T> {
    Ref(&'a T),
    Owned(T),
}

impl<T> Guarded<'_, T> {
    #[inline]
    fn as_ref(&self) -> &T {
        match self {
            Guarded::Ref(val) => val,
            Guarded::Owned(val) => val,
        }
    }
}

/// Guard the inner value, once this value is dropped the inner value is also removed.
///
/// This type guarantees that it never outlives the wrapped value.
#[derive(Debug)]
#[must_use]
pub(crate) struct GcGuard<'a, Val> {
    inner: Rc<RefCell<Option<Guarded<'a, Val>>>>,
}

impl<'a, Val> Drop for GcGuard<'a, Val> {
    fn drop(&mut self) {
        self.inner.borrow_mut().take();
    }
}

/// The Log object that is passed to the javascript inspector.
#[derive(Debug)]
pub(crate) struct StepLog {
    /// Stack before step execution
    pub(crate) stack: StackRef,
    /// Opcode to be executed
    pub(crate) op: OpObj,
    /// All allocated memory in a step
    pub(crate) memory: MemoryRef,
    /// Program counter before step execution
    pub(crate) pc: u64,
    /// Remaining gas before step execution
    pub(crate) gas_remaining: u64,
    /// Gas cost of step execution
    pub(crate) cost: u64,
    /// Call depth
    pub(crate) depth: u64,
    /// Gas refund counter before step execution
    pub(crate) refund: u64,
    /// returns information about the error if one occurred, otherwise returns undefined
    pub(crate) error: Option<String>,
    /// The contract object available to the js inspector
    pub(crate) contract: Contract,
}

impl StepLog {
    /// Converts the contract object into a js object
    ///
    /// Caution: this expects a global property `bigint` to be present.
    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let Self {
            stack,
            op,
            memory,
            pc,
            gas_remaining: gas,
            cost,
            depth,
            refund,
            error,
            contract,
        } = self;
        let obj = JsObject::default();

        // fields
        let op = op.into_js_object(ctx)?;
        let memory = memory.into_js_object(ctx)?;
        let stack = stack.into_js_object(ctx)?;
        let contract = contract.into_js_object(ctx)?;

        obj.set(js_string!("op"), op, false, ctx)?;
        obj.set(js_string!("memory"), memory, false, ctx)?;
        obj.set(js_string!("stack"), stack, false, ctx)?;
        obj.set(js_string!("contract"), contract, false, ctx)?;

        // methods
        let error = if let Some(error) = error {
            JsValue::from(js_string!(error))
        } else {
            JsValue::undefined()
        };
        let get_error = js_value_capture_getter!(error, ctx);
        let get_pc = js_value_getter!(pc, ctx);
        let get_gas = js_value_getter!(gas, ctx);
        let get_cost = js_value_getter!(cost, ctx);
        let get_refund = js_value_getter!(refund, ctx);
        let get_depth = js_value_getter!(depth, ctx);

        obj.set(js_string!("getPC"), get_pc, false, ctx)?;
        obj.set(js_string!("getError"), get_error, false, ctx)?;
        obj.set(js_string!("getGas"), get_gas, false, ctx)?;
        obj.set(js_string!("getCost"), get_cost, false, ctx)?;
        obj.set(js_string!("getDepth"), get_depth, false, ctx)?;
        obj.set(js_string!("getRefund"), get_refund, false, ctx)?;

        Ok(obj)
    }
}

/// Represents the memory object
#[derive(Clone, Debug)]
pub(crate) struct MemoryRef(GuardedNullableGc<SharedMemory>);

impl MemoryRef {
    /// Creates a new stack reference
    pub(crate) fn new(mem: &SharedMemory) -> (Self, GcGuard<'_, SharedMemory>) {
        let (inner, guard) = GuardedNullableGc::new_ref(mem);
        (Self(inner), guard)
    }

    fn len(&self) -> usize {
        self.0.with_inner(|mem| mem.len()).unwrap_or_default()
    }

    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let obj = JsObject::default();
        let len = self.len();

        let length = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, _ctx| {
                Ok(JsValue::from(len as u64))
            }),
        )
        .length(0)
        .build();

        // slice returns the requested range of memory as a byte slice.
        let slice = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, memory, ctx| {
                    let start = args.get_or_undefined(0).to_number(ctx)?;
                    let end = args.get_or_undefined(1).to_number(ctx)?;
                    if end < start || start < 0. || (end as usize) < memory.len() {
                        return Err(JsError::from_native(JsNativeError::typ().with_message(
                            format!(
                                "tracer accessed out of bound memory: offset {start}, end {end}"
                            ),
                        )));
                    }
                    let start = start as usize;
                    let end = end as usize;
                    let size = end - start;
                    let slice = memory
                        .0
                        .with_inner(|mem| mem.slice(start, size).to_vec())
                        .unwrap_or_default();

                    to_buf_value(slice, ctx)
                },
                self.clone(),
            ),
        )
        .length(2)
        .build();

        let get_uint = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, memory, ctx|  {
                    let offset_f64 = args.get_or_undefined(0).to_number(ctx)?;
                     let len = memory.len();
                     let offset = offset_f64 as usize;
                     if len < offset+32 || offset_f64 < 0. {
                         return Err(JsError::from_native(
                             JsNativeError::typ().with_message(format!("tracer accessed out of bound memory: available {len}, offset {offset}, size 32"))
                         ));
                     }
                    let slice = memory.0.with_inner(|mem| mem.slice(offset, 32).to_vec()).unwrap_or_default();
                     to_buf_value(slice, ctx)
                },
                 self
            ),
        )
            .length(1)
            .build();

        obj.set(js_string!("slice"), slice, false, ctx)?;
        obj.set(js_string!("getUint"), get_uint, false, ctx)?;
        obj.set(js_string!("length"), length, false, ctx)?;
        Ok(obj)
    }
}

impl Finalize for MemoryRef {}

unsafe impl Trace for MemoryRef {
    empty_trace!();
}

/// Represents the state object
#[derive(Clone, Debug)]
pub(crate) struct StateRef(GuardedNullableGc<EvmState>);

impl StateRef {
    /// Creates a new stack reference
    pub(crate) fn new(state: &EvmState) -> (Self, GcGuard<'_, EvmState>) {
        let (inner, guard) = GuardedNullableGc::new_ref(state);
        (Self(inner), guard)
    }

    fn get_account(&self, address: &Address) -> Option<AccountInfo> {
        self.0.with_inner(|state| state.get(address).map(|acc| acc.info.clone()))?
    }
}

impl Finalize for StateRef {}

unsafe impl Trace for StateRef {
    empty_trace!();
}

/// Represents the database
#[derive(Clone, Debug)]
pub(crate) struct GcDb<DB: 'static>(GuardedNullableGc<DB>);

impl<DB> GcDb<DB>
where
    DB: DatabaseRef + 'static,
{
    /// Creates a new stack reference
    fn new<'a>(db: DB) -> (Self, GcGuard<'a, DB>) {
        let (inner, guard) = GuardedNullableGc::new_owned(db);
        (Self(inner), guard)
    }
}

impl<DB: 'static> Finalize for GcDb<DB> {}

unsafe impl<DB: 'static> Trace for GcDb<DB> {
    empty_trace!();
}

/// Represents the opcode object
#[derive(Debug)]
pub(crate) struct OpObj(pub(crate) u8);

impl OpObj {
    pub(crate) fn into_js_object(self, context: &mut Context) -> JsResult<JsObject> {
        let obj = JsObject::default();
        let value = self.0;
        let is_push = (PUSH0..=PUSH32).contains(&value);

        let to_number = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, _ctx| Ok(JsValue::from(value))),
        )
        .length(0)
        .build();

        let is_push = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, _ctx| Ok(JsValue::from(is_push))),
        )
        .length(0)
        .build();

        let to_string = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, _ctx| {
                // we always want an OpCode, even it is unknown because it could be an additional
                // opcode that not a known constant
                let op = unsafe { OpCode::new_unchecked(value) };
                let s = op.to_string();
                Ok(JsValue::from(js_string!(s)))
            }),
        )
        .length(0)
        .build();

        obj.set(js_string!("toNumber"), to_number, false, context)?;
        obj.set(js_string!("toString"), to_string, false, context)?;
        obj.set(js_string!("isPush"), is_push, false, context)?;
        Ok(obj)
    }
}

impl From<u8> for OpObj {
    fn from(op: u8) -> Self {
        Self(op)
    }
}

/// Represents the stack object
#[derive(Debug)]
pub(crate) struct StackRef(GuardedNullableGc<Stack>);

impl StackRef {
    /// Creates a new stack reference
    pub(crate) fn new(stack: &Stack) -> (Self, GcGuard<'_, Stack>) {
        let (inner, guard) = GuardedNullableGc::new_ref(stack);
        (Self(inner), guard)
    }

    fn peek(&self, idx: usize, ctx: &mut Context) -> JsResult<JsValue> {
        self.0
            .with_inner(|stack| {
                let value = stack.peek(idx).map_err(|_| {
                    JsError::from_native(JsNativeError::typ().with_message(format!(
                        "tracer accessed out of bound stack: size {}, index {}",
                        stack.len(),
                        idx
                    )))
                })?;
                to_bigint(value, ctx)
            })
            .ok_or_else(|| {
                JsError::from_native(JsNativeError::typ().with_message(format!(
                    "tracer accessed out of bound stack: size 0, index {}",
                    idx
                )))
            })?
    }

    pub(crate) fn into_js_object(self, context: &mut Context) -> JsResult<JsObject> {
        let obj = JsObject::default();
        let len = self.0.with_inner(|stack| stack.len()).unwrap_or_default();
        let length = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, _ctx| Ok(JsValue::from(len))),
        )
        .length(0)
        .build();

        // peek returns the nth-from-the-top element of the stack.
        let peek = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, stack, ctx| {
                    let idx_f64 = args.get_or_undefined(0).to_number(ctx)?;
                    let idx = idx_f64 as usize;
                    if len <= idx || idx_f64 < 0. {
                        return Err(JsError::from_native(JsNativeError::typ().with_message(
                            format!(
                                "tracer accessed out of bound stack: size {len}, index {idx_f64}"
                            ),
                        )));
                    }
                    stack.peek(idx, ctx)
                },
                self,
            ),
        )
        .length(1)
        .build();

        obj.set(js_string!("length"), length, false, context)?;
        obj.set(js_string!("peek"), peek, false, context)?;
        Ok(obj)
    }
}

impl Finalize for StackRef {}

unsafe impl Trace for StackRef {
    empty_trace!();
}

/// Represents the contract object
#[derive(Clone, Debug, Default)]
pub(crate) struct Contract {
    pub(crate) caller: Address,
    pub(crate) contract: Address,
    pub(crate) value: U256,
    pub(crate) input: Bytes,
}

impl Contract {
    /// Converts the contract object into a js object
    ///
    /// Caution: this expects a global property `bigint` to be present.
    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let Self { caller, contract, value, input } = self;
        let obj = JsObject::default();

        let get_caller = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, ctx| {
                to_buf_value(caller.as_slice().to_vec(), ctx)
            }),
        )
        .length(0)
        .build();

        let get_address = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, ctx| {
                to_buf_value(contract.as_slice().to_vec(), ctx)
            }),
        )
        .length(0)
        .build();

        let get_value = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, ctx| to_bigint(value, ctx)),
        )
        .length(0)
        .build();

        let input = to_buf_value(input.to_vec(), ctx)?;
        let get_input = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, _args, input, _ctx| Ok(input.clone()),
                input,
            ),
        )
        .length(0)
        .build();

        obj.set(js_string!("getCaller"), get_caller, false, ctx)?;
        obj.set(js_string!("getAddress"), get_address, false, ctx)?;
        obj.set(js_string!("getValue"), get_value, false, ctx)?;
        obj.set(js_string!("getInput"), get_input, false, ctx)?;

        Ok(obj)
    }
}

/// Represents the call frame object for exit functions
pub(crate) struct FrameResult {
    pub(crate) gas_used: u64,
    pub(crate) output: Bytes,
    pub(crate) error: Option<String>,
}

impl FrameResult {
    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let Self { gas_used, output, error } = self;
        let obj = JsObject::default();

        let output = to_buf_value(output.to_vec(), ctx)?;
        let get_output = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, _args, output, _ctx| Ok(output.clone()),
                output,
            ),
        )
        .length(0)
        .build();

        let error = error.map(|err| JsValue::from(js_string!(err))).unwrap_or_default();
        let get_error = js_value_capture_getter!(error, ctx);
        let get_gas_used = js_value_getter!(gas_used, ctx);

        obj.set(js_string!("getGasUsed"), get_gas_used, false, ctx)?;
        obj.set(js_string!("getOutput"), get_output, false, ctx)?;
        obj.set(js_string!("getError"), get_error, false, ctx)?;

        Ok(obj)
    }
}

/// Represents the call frame object for enter functions
pub(crate) struct CallFrame {
    pub(crate) contract: Contract,
    pub(crate) kind: CallKind,
    pub(crate) gas: u64,
}

impl CallFrame {
    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let Self { contract: Contract { caller, contract, value, input }, kind, gas } = self;
        let obj = JsObject::default();

        let get_from = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, ctx| {
                to_buf_value(caller.as_slice().to_vec(), ctx)
            }),
        )
        .length(0)
        .build();

        let get_to = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, ctx| {
                to_buf_value(contract.as_slice().to_vec(), ctx)
            }),
        )
        .length(0)
        .build();

        let get_value = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure(move |_this, _args, ctx| to_bigint(value, ctx)),
        )
        .length(0)
        .build();

        let input = to_buf_value(input.to_vec(), ctx)?;
        let get_input = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, _args, input, _ctx| Ok(input.clone()),
                input,
            ),
        )
        .length(0)
        .build();

        let get_gas = js_value_getter!(gas, ctx);
        let ty = js_string!(kind.to_string());
        let get_type = js_value_capture_getter!(ty, ctx);

        obj.set(js_string!("getFrom"), get_from, false, ctx)?;
        obj.set(js_string!("getTo"), get_to, false, ctx)?;
        obj.set(js_string!("getValue"), get_value, false, ctx)?;
        obj.set(js_string!("getInput"), get_input, false, ctx)?;
        obj.set(js_string!("getGas"), get_gas, false, ctx)?;
        obj.set(js_string!("getType"), get_type, false, ctx)?;

        Ok(obj)
    }
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
    pub(crate) output: Bytes,
    /// Number, block number
    pub(crate) time: String,
    pub(crate) transaction_ctx: TransactionContext,
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
            output,
            time,
            transaction_ctx,
        } = self;
        let obj = JsObject::default();

        // add properties

        obj.set(js_string!("type"), js_string!(r#type), false, ctx)?;
        obj.set(js_string!("from"), address_to_buf(from, ctx)?, false, ctx)?;
        if let Some(to) = to {
            obj.set(js_string!("to"), address_to_buf(to, ctx)?, false, ctx)?;
        } else {
            obj.set(js_string!("to"), JsValue::null(), false, ctx)?;
        }

        obj.set(js_string!("input"), to_buf(input.to_vec(), ctx)?, false, ctx)?;
        obj.set(js_string!("gas"), gas, false, ctx)?;
        obj.set(js_string!("gasUsed"), gas_used, false, ctx)?;
        obj.set(js_string!("gasPrice"), gas_price, false, ctx)?;
        obj.set(js_string!("intrinsicGas"), intrinsic_gas, false, ctx)?;
        obj.set(js_string!("value"), to_bigint(value, ctx)?, false, ctx)?;
        obj.set(js_string!("block"), block, false, ctx)?;
        obj.set(js_string!("output"), to_buf(output.to_vec(), ctx)?, false, ctx)?;
        obj.set(js_string!("time"), js_string!(time), false, ctx)?;
        if let Some(block_hash) = transaction_ctx.block_hash {
            obj.set(
                js_string!("blockHash"),
                to_buf(block_hash.as_slice().to_vec(), ctx)?,
                false,
                ctx,
            )?;
        }
        if let Some(tx_index) = transaction_ctx.tx_index {
            obj.set(js_string!("txIndex"), tx_index as u64, false, ctx)?;
        }
        if let Some(tx_hash) = transaction_ctx.tx_hash {
            obj.set(js_string!("txHash"), to_buf(tx_hash.as_slice().to_vec(), ctx)?, false, ctx)?;
        }

        Ok(obj)
    }
}

/// DB is the object that allows the js inspector to interact with the database.
#[derive(Clone)]
pub(crate) struct EvmDbRef {
    inner: Rc<EvmDbRefInner>,
}

impl EvmDbRef {
    /// Creates a new evm and db JS object.
    pub(crate) fn new<'a, 'b, DB>(state: &'a EvmState, db: &'b DB) -> (Self, EvmDbGuard<'a, 'b>)
    where
        DB: DatabaseRef,
        DB::Error: std::fmt::Display,
    {
        let (state, state_guard) = StateRef::new(state);

        // SAFETY:
        //
        // boa requires 'static lifetime for all objects.
        // As mentioned in the `Safety` section of [GuardedNullableGc] the caller of this function
        // needs to guarantee that the passed-in lifetime is sufficiently long for the lifetime of
        // the guard.
        let db = JsDb(db);
        let js_db = unsafe {
            std::mem::transmute::<
                Box<dyn DatabaseRef<Error = String> + '_>,
                Box<dyn DatabaseRef<Error = String> + 'static>,
            >(Box::new(db))
        };

        let (db, db_guard) = GcDb::new(js_db);

        let inner = EvmDbRefInner { state, db };
        let this = Self { inner: Rc::new(inner) };
        let guard = EvmDbGuard { _state_guard: state_guard, _db_guard: db_guard };
        (this, guard)
    }

    fn read_basic(&self, address: JsValue, ctx: &mut Context) -> JsResult<Option<AccountInfo>> {
        let buf = from_buf(address, ctx)?;
        let address = bytes_to_address(buf);
        if let acc @ Some(_) = self.inner.state.get_account(&address) {
            return Ok(acc);
        }

        let res = self.inner.db.0.with_inner(|db| db.basic_ref(address));
        match res {
            Some(Ok(maybe_acc)) => Ok(maybe_acc),
            _ => Err(JsError::from_native(
                JsNativeError::error()
                    .with_message(format!("Failed to read address {address:?} from database",)),
            )),
        }
    }

    fn read_code(&self, address: JsValue, ctx: &mut Context) -> JsResult<JsArrayBuffer> {
        let acc = self.read_basic(address, ctx)?;
        let code_hash = acc.map(|acc| acc.code_hash).unwrap_or(KECCAK_EMPTY);
        if code_hash == KECCAK_EMPTY {
            return JsArrayBuffer::new(0, ctx);
        }

        let Some(Ok(bytecode)) = self.inner.db.0.with_inner(|db| db.code_by_hash_ref(code_hash))
        else {
            return Err(JsError::from_native(
                JsNativeError::error()
                    .with_message(format!("Failed to read code hash {code_hash:?} from database")),
            ));
        };

        to_buf(bytecode.bytecode().to_vec(), ctx)
    }

    fn read_state(
        &self,
        address: JsValue,
        slot: JsValue,
        ctx: &mut Context,
    ) -> JsResult<JsArrayBuffer> {
        let buf = from_buf(address, ctx)?;
        let address = bytes_to_address(buf);

        let buf = from_buf(slot, ctx)?;
        let slot = bytes_to_hash(buf);

        let res = self.inner.db.0.with_inner(|db| db.storage_ref(address, slot.into()));

        let value = match res {
            Some(Ok(value)) => value,
            _ => {
                return Err(JsError::from_native(JsNativeError::error().with_message(format!(
                    "Failed to read state for {address:?} at {slot:?} from database",
                ))))
            }
        };
        let value: B256 = value.into();
        to_buf(value.as_slice().to_vec(), ctx)
    }

    pub(crate) fn into_js_object(self, ctx: &mut Context) -> JsResult<JsObject> {
        let obj = JsObject::default();
        let exists = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, db, ctx| {
                    let val = args.get_or_undefined(0).clone();
                    let acc = db.read_basic(val, ctx)?;
                    let exists = acc.is_some();
                    Ok(JsValue::from(exists))
                },
                self.clone(),
            ),
        )
        .length(1)
        .build();

        let get_balance = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, db, ctx| {
                    let val = args.get_or_undefined(0).clone();
                    let acc = db.read_basic(val, ctx)?;
                    let balance = acc.map(|acc| acc.balance).unwrap_or_default();
                    to_bigint(balance, ctx)
                },
                self.clone(),
            ),
        )
        .length(1)
        .build();

        let get_nonce = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, db, ctx| {
                    let val = args.get_or_undefined(0).clone();
                    let acc = db.read_basic(val, ctx)?;
                    let nonce = acc.map(|acc| acc.nonce).unwrap_or_default();
                    Ok(JsValue::from(nonce))
                },
                self.clone(),
            ),
        )
        .length(1)
        .build();

        let get_code = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, db, ctx| {
                    let val = args.get_or_undefined(0).clone();
                    Ok(db.read_code(val, ctx)?.into())
                },
                self.clone(),
            ),
        )
        .length(1)
        .build();

        let get_state = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, db, ctx| {
                    let addr = args.get_or_undefined(0).clone();
                    let slot = args.get_or_undefined(1).clone();
                    Ok(db.read_state(addr, slot, ctx)?.into())
                },
                self,
            ),
        )
        .length(2)
        .build();

        obj.set(js_string!("getBalance"), get_balance, false, ctx)?;
        obj.set(js_string!("getNonce"), get_nonce, false, ctx)?;
        obj.set(js_string!("getCode"), get_code, false, ctx)?;
        obj.set(js_string!("getState"), get_state, false, ctx)?;
        obj.set(js_string!("exists"), exists, false, ctx)?;
        Ok(obj)
    }
}

impl Finalize for EvmDbRef {}

unsafe impl Trace for EvmDbRef {
    empty_trace!();
}

/// DB is the object that allows the js inspector to interact with the database.
struct EvmDbRefInner {
    state: StateRef,
    db: GcDb<Box<dyn DatabaseRef<Error = String> + 'static>>,
}

/// Guard the inner references, once this value is dropped the inner reference is also removed.
///
/// This ensures that the guards are dropped within the lifetime of the borrowed values.
#[must_use]
pub(crate) struct EvmDbGuard<'a, 'b> {
    _state_guard: GcGuard<'a, EvmState>,
    _db_guard: GcGuard<'b, Box<dyn DatabaseRef<Error = String> + 'static>>,
}

/// A wrapper Database for the JS context.
pub(crate) struct JsDb<DB: DatabaseRef>(DB);

impl<DB> DatabaseRef for JsDb<DB>
where
    DB: DatabaseRef,
    DB::Error: std::fmt::Display,
{
    type Error = String;

    fn basic_ref(&self, _address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.0.basic_ref(_address).map_err(|e| e.to_string())
    }

    fn code_by_hash_ref(&self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.0.code_by_hash_ref(_code_hash).map_err(|e| e.to_string())
    }

    fn storage_ref(&self, _address: Address, _index: U256) -> Result<U256, Self::Error> {
        self.0.storage_ref(_address, _index).map_err(|e| e.to_string())
    }

    fn block_hash_ref(&self, _number: U256) -> Result<B256, Self::Error> {
        self.0.block_hash_ref(_number).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing::js::builtins::{
        json_stringify, register_builtins, to_serde_value, BIG_INT_JS,
    };
    use boa_engine::{property::Attribute, Source};
    use revm::db::{CacheDB, EmptyDB};

    #[test]
    fn test_contract() {
        let mut ctx = Context::default();
        let contract = Contract {
            caller: Address::ZERO,
            contract: Address::ZERO,
            value: U256::from(1337u64),
            input: vec![0x01, 0x02, 0x03].into(),
        };
        let big_int = ctx.eval(Source::from_bytes(BIG_INT_JS)).unwrap();
        ctx.register_global_property(js_string!("bigint"), big_int, Attribute::all()).unwrap();

        let obj = contract.clone().into_js_object(&mut ctx).unwrap();
        let s = "({
                caller: function(contract) { return contract.getCaller(); },
                value: function(contract) { return contract.getValue(); },
                address: function(contract) { return contract.getAddress(); },
                input: function(contract) { return contract.getInput(); }
        })";

        let contract_arg = JsValue::from(obj);
        let eval_obj = ctx.eval(Source::from_bytes(s)).unwrap();
        let call = eval_obj.as_object().unwrap().get(js_string!("caller"), &mut ctx).unwrap();
        let res = call
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), &[contract_arg.clone()], &mut ctx)
            .unwrap();
        assert!(res.is_object());
        let obj = res.as_object().unwrap();
        let array_buf = JsArrayBuffer::from_object(obj.clone());
        assert!(array_buf.is_ok());

        let get_address =
            eval_obj.as_object().unwrap().get(js_string!("address"), &mut ctx).unwrap();
        let res = get_address
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), &[contract_arg.clone()], &mut ctx)
            .unwrap();
        assert!(res.is_object());
        let obj = res.as_object().unwrap();
        let array_buf = JsArrayBuffer::from_object(obj.clone()).unwrap();
        assert_eq!(array_buf.data().unwrap().to_vec(), contract.contract.as_slice());

        let call = eval_obj.as_object().unwrap().get(js_string!("value"), &mut ctx).unwrap();
        let res = call
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), &[contract_arg.clone()], &mut ctx)
            .unwrap();
        assert_eq!(
            res.to_string(&mut ctx).unwrap().to_std_string().unwrap(),
            contract.value.to_string()
        );

        let call = eval_obj.as_object().unwrap().get(js_string!("input"), &mut ctx).unwrap();
        let res = call
            .as_callable()
            .unwrap()
            .call(&JsValue::undefined(), &[contract_arg], &mut ctx)
            .unwrap();

        let buffer = JsArrayBuffer::from_object(res.as_object().unwrap().clone()).unwrap();
        let input = buffer.data().unwrap().to_vec();
        assert_eq!(input, contract.input);
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
        {
            let (db, guard) = EvmDbRef::new(&state, &db);
            let addr = Address::default();
            let addr = JsValue::from(js_string!(addr.to_string()));
            let db = db.into_js_object(&mut context).unwrap();
            let res = f.call(&result, &[db.clone().into(), addr.clone()], &mut context).unwrap();
            assert!(!res.as_boolean().unwrap());

            // drop the db which also drops any GC values
            drop(guard);
            let res = f.call(&result, &[db.clone().into(), addr.clone()], &mut context);
            assert!(res.is_err());
        }
        let addr = Address::default();
        db.insert_account_info(addr, Default::default());

        {
            let (db, guard) = EvmDbRef::new(&state, &db);
            let addr = JsValue::from(js_string!(addr.to_string()));
            let db = db.into_js_object(&mut context).unwrap();
            let res = f.call(&result, &[db.clone().into(), addr.clone()], &mut context).unwrap();

            // account exists
            assert!(res.as_boolean().unwrap());

            // drop the db which also drops any GC values
            drop(guard);
            let res = f.call(&result, &[db.clone().into(), addr.clone()], &mut context);
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

        let result_fn =
            obj.get(js_string!("result"), &mut context).unwrap().as_object().cloned().unwrap();
        let setup_fn =
            obj.get(js_string!("setup"), &mut context).unwrap().as_object().cloned().unwrap();

        let db = CacheDB::new(EmptyDB::new());
        let state = EvmState::default();
        {
            let (db_ref, guard) = EvmDbRef::new(&state, &db);
            let js_db = db_ref.into_js_object(&mut context).unwrap();
            let _res = setup_fn.call(&(obj.clone().into()), &[js_db.into()], &mut context).unwrap();
            assert!(obj.get(js_string!("db"), &mut context).unwrap().is_object());

            let addr = Address::default();
            let addr = JsValue::from(js_string!(addr.to_string()));
            let res = result_fn.call(&(obj.clone().into()), &[addr.clone()], &mut context).unwrap();
            assert!(!res.as_boolean().unwrap());

            // drop the guard which also drops any GC values
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

        let result_fn =
            obj.get(js_string!("result"), &mut context).unwrap().as_object().cloned().unwrap();
        let step_fn =
            obj.get(js_string!("step"), &mut context).unwrap().as_object().cloned().unwrap();

        let mut stack = Stack::new();
        stack.push(U256::from(35000)).unwrap();
        stack.push(U256::from(35000)).unwrap();
        stack.push(U256::from(35000)).unwrap();
        let (stack_ref, _stack_guard) = StackRef::new(&stack);
        let mem = SharedMemory::new();
        let (mem_ref, _mem_guard) = MemoryRef::new(&mem);

        let step = StepLog {
            stack: stack_ref,
            op: OpObj(0),
            memory: mem_ref,
            pc: 0,
            gas_remaining: 0,
            cost: 0,
            depth: 0,
            refund: 0,
            error: None,
            contract: Default::default(),
        };

        let js_step = step.into_js_object(&mut context).unwrap();

        let _ = step_fn.call(&eval, &[js_step.into()], &mut context).unwrap();

        let res = result_fn.call(&eval, &[], &mut context).unwrap();
        let val = json_stringify(res.clone(), &mut context).unwrap().to_std_string().unwrap();
        assert_eq!(val, r#"[{"value":"35000"}]"#);

        let val = to_serde_value(res, &mut context).unwrap();
        assert!(val.is_array());
        let s = val.to_string();
        assert_eq!(s, r#"[{"value":"35000"}]"#);
    }
}
