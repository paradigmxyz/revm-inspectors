//! Step buffer capture and reuse tests.

use alloy_primitives::{hex, Address, Bytes, TxKind};
use alloy_rpc_types_trace::geth::GethDefaultTracingOptions;
use revm::{
    bytecode::{opcode::OpCode, Bytecode},
    context::TxEnv,
    context_interface::ContextTr,
    database::CacheDB,
    database_interface::EmptyDB,
    inspector::JournalExt,
    interpreter::{interpreter_types::Jumps, CallInputs, CallOutcome, Interpreter},
    state::AccountInfo,
    Context, InspectEvm, Inspector, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{OpcodeFilter, TracingInspector, TracingInspectorConfig};
use std::num::NonZeroU64;

const CONTRACT: Address = Address::repeat_byte(0x11);
const CHILD: Address = Address::with_last_byte(0xff);

fn database() -> CacheDB<EmptyDB> {
    let mut db = CacheDB::new(EmptyDB::default());
    // MSTORE(0, 0x2a), STATICCALL child, MSTORE(0, 0x63), PUSH1 POP STOP.
    // The child returns a nonempty buffer, and the parent modifies memory while capture can
    // be disabled. Reenabling capture after the final PUSH1 must not reuse stale memory.
    for (address, code) in [
        (
            CONTRACT,
            Bytes::from_static(&hex!("602a600052602060006020600060ff61fffffa50606360005260015000")),
        ),
        (CHILD, Bytes::from_static(&hex!("602b60005260206000f3"))),
    ] {
        db.insert_account_info(
            address,
            AccountInfo { code: Some(Bytecode::new_raw(code)), ..Default::default() },
        );
    }
    db
}

fn tx() -> TxEnv {
    TxEnv { kind: TxKind::Call(CONTRACT), gas_limit: 200_000, ..Default::default() }
}

#[test]
fn step_buffers_follow_capture_changes() {
    let schedules: [fn(usize) -> bool; 5] =
        [|_| true, |_| false, |pc| pc >= 5, |pc| pc < 20, |pc| (5..20).contains(&pc) || pc >= 27];
    for filter in [
        None,
        Some(
            OpcodeFilter::new()
                .enabled(OpCode::MSTORE)
                .enabled(OpCode::STATICCALL)
                .enabled(OpCode::POP)
                .enabled(OpCode::RETURN)
                .enabled(OpCode::STOP),
        ),
    ] {
        for limit in [0, 7, 12, 100] {
            let config = TracingInspectorConfig {
                record_opcodes_filter: filter,
                step_limit: NonZeroU64::new(limit),
                ..TracingInspectorConfig::all()
            };
            let mut reference = Context::mainnet()
                .with_db(database())
                .build_mainnet_with_inspector(TracingInspector::new(config));
            let result = reference.inspect_tx(tx()).unwrap().result;
            assert!(result.is_success());

            for capture_at in schedules {
                // Exercise each capture on its own and every combination of captures.
                for mask in 1..8 {
                    let mut evm = Context::mainnet()
                        .with_db(database())
                        .build_mainnet_with_inspector(CaptureInspector {
                            inner: TracingInspector::new(config),
                            capture_at,
                            mask,
                        });
                    assert_eq!(evm.inspect_tx(tx()).unwrap().result, result);
                    let nodes = evm.inspector.inner.traces().nodes();
                    let expected = reference.inspector.traces().nodes();
                    assert_eq!(nodes.len(), expected.len());
                    for (node, reference) in nodes.iter().zip(expected) {
                        assert_eq!(node.trace.steps, reference.trace.steps);
                        let has_buffers = node.trace.steps.iter().any(|step| capture_at(step.pc));
                        assert_eq!(
                            node.trace.step_buffers.len(),
                            if has_buffers { node.trace.steps.len() } else { 0 },
                        );
                        for (idx, step) in node.trace.steps.iter().enumerate() {
                            let mut expected = reference.trace.buffers_at(idx).unwrap().clone();
                            if !capture_at(step.pc) || mask & 1 == 0 {
                                expected.memory = None;
                            }
                            if !capture_at(step.pc) || mask & 2 == 0 {
                                expected.returndata = Bytes::new();
                            }
                            if !capture_at(step.pc) || mask & 4 == 0 {
                                expected.immediate_bytes = None;
                            }
                            assert_eq!(
                                node.trace.buffers_at(idx).cloned().unwrap_or_default(),
                                expected,
                                "mask={mask}, pc={}, depth={}",
                                step.pc,
                                node.trace.depth,
                            );
                        }
                    }

                    let opts = GethDefaultTracingOptions::default()
                        .with_enable_memory(true)
                        .with_enable_return_data(true);
                    let mut expected = reference.inspector.geth_builder().geth_traces(
                        result.tx_gas_used(),
                        Bytes::new(),
                        opts,
                    );
                    for log in &mut expected.struct_logs {
                        if !capture_at(log.pc as usize) || mask & 1 == 0 {
                            log.memory = None;
                        }
                        if !capture_at(log.pc as usize) || mask & 2 == 0 {
                            log.return_data = Some(Bytes::new());
                        }
                    }
                    assert_eq!(
                        evm.inspector.inner.geth_builder().geth_traces(
                            result.tx_gas_used(),
                            Bytes::new(),
                            opts,
                        ),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn fuse_reuses_buffer_capacity_without_retaining_snapshots() {
    let mut evm = Context::mainnet()
        .with_db(database())
        .build_mainnet_with_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    assert!(evm.inspect_tx(tx()).unwrap().result.is_success());
    let mut original = evm
        .inspector
        .traces()
        .nodes()
        .iter()
        .map(|node| (node.trace.step_buffers.as_ptr(), node.trace.step_buffers.capacity()))
        .collect::<Vec<_>>();
    assert!(original.iter().all(|(_, capacity)| *capacity > 0));
    original.sort_unstable();

    let expected = evm.inspector.traces().clone();
    evm.inspector.fuse();
    *evm.inspector.config_mut() = TracingInspectorConfig::default_geth();
    assert!(evm.inspect_tx(tx()).unwrap().result.is_success());
    let mut reused = evm
        .inspector
        .traces()
        .nodes()
        .iter()
        .map(|node| {
            assert!(node.trace.step_buffers.is_empty());
            (node.trace.step_buffers.as_ptr(), node.trace.step_buffers.capacity())
        })
        .collect::<Vec<_>>();
    reused.sort_unstable();
    assert_eq!(reused, original);

    evm.inspector.fuse();
    *evm.inspector.config_mut() = TracingInspectorConfig::all();
    assert!(evm.inspect_tx(tx()).unwrap().result.is_success());
    assert_eq!(evm.inspector.traces(), &expected);
}

#[cfg(feature = "serde")]
#[test]
fn step_buffers_serde_roundtrip() {
    let mut evm = Context::mainnet()
        .with_db(database())
        .build_mainnet_with_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    assert!(evm.inspect_tx(tx()).unwrap().result.is_success());
    let arena = evm.inspector.traces();
    let json = serde_json::to_string(arena).unwrap();
    assert_eq!(
        &serde_json::from_str::<revm_inspectors::tracing::CallTraceArena>(&json).unwrap(),
        arena
    );
}

#[cfg(feature = "serde")]
#[test]
fn traces_without_buffers_accept_missing_step_buffers() {
    let mut evm = Context::mainnet().with_db(database()).build_mainnet_with_inspector(
        TracingInspector::new(TracingInspectorConfig::default_geth()),
    );
    assert!(evm.inspect_tx(tx()).unwrap().result.is_success());
    for node in evm.inspector.traces().nodes() {
        let mut json = serde_json::to_value(&node.trace).unwrap();
        json.as_object_mut().unwrap().remove("step_buffers");
        for step in json["steps"].as_array_mut().unwrap() {
            let step = step.as_object_mut().unwrap();
            step.insert("memory".into(), serde_json::Value::Null);
            step.insert("returndata".into(), serde_json::json!("0x"));
            step.insert("immediate_bytes".into(), serde_json::Value::Null);
        }
        assert_eq!(
            serde_json::from_value::<revm_inspectors::tracing::types::CallTrace>(json).unwrap(),
            node.trace
        );
    }
}

#[derive(Debug)]
struct CaptureInspector {
    inner: TracingInspector,
    capture_at: fn(usize) -> bool,
    mask: u8,
}

impl<CTX: ContextTr<Journal: JournalExt>> Inspector<CTX> for CaptureInspector {
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        self.inner.initialize_interp(interp, context);
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.inner.call(context, inputs)
    }

    fn call_end(&mut self, context: &mut CTX, inputs: &CallInputs, outcome: &mut CallOutcome) {
        self.inner.call_end(context, inputs, outcome);
    }

    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        let capture = (self.capture_at)(interp.bytecode.pc());
        let config = self.inner.config_mut();
        config.record_memory_snapshots = capture && self.mask & 1 != 0;
        config.record_returndata_snapshots = capture && self.mask & 2 != 0;
        config.record_immediate_bytes = capture && self.mask & 4 != 0;
        self.inner.step(interp, context);
    }

    fn step_end(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        self.inner.step_end(interp, context);
    }
}
