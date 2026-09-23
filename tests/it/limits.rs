//! Byte budgets fail tracing without changing EVM execution.

use alloy_primitives::{hex, Address, Bytes};
use alloy_rpc_types_trace::geth::{CallConfig, GethDebugTracingOptions};
use revm::{
    bytecode::Bytecode,
    context::TxEnv,
    context_interface::{result::ExecutionResult, ContextTr, TransactTo},
    database::CacheDB,
    database_interface::EmptyDB,
    inspector::InspectorEvmTr,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{
    types::{CallLog, CallTraceNode, CallTraceStep, StepDelta, StorageChange, TraceMemberOrder},
    DebugInspector, DebugInspectorError, MuxError, StackSnapshotType, TraceError, TraceLimits,
    TracingInspector, TracingInspectorConfig,
};

const TARGET: Address = Address::with_last_byte(0x42);
const CHILD: Address = Address::with_last_byte(0x43);
const BIG_CALL: &[u8] = &hex!("5f5f620100005f5f604361fffff100");
// Five 1 KiB calls, followed by a return to exercise callbacks after exhaustion.
const MANY_CALLS: &[u8] = &hex!("60055b5f5f6104005f5f604361fffff150600190038060025760205ff3");
const CREATE: &[u8] = &hex!("6960fe5f5360015ff300005f52600a60165ff000");

fn database(code: &[u8], child: &[u8]) -> CacheDB<EmptyDB> {
    let mut db = CacheDB::default();
    for (address, code) in [(TARGET, code), (CHILD, child)] {
        db.insert_account_info(
            address,
            AccountInfo::default().with_code(Bytecode::new_raw(code.to_vec().into())),
        );
    }
    db
}

fn run(
    code: &[u8],
    child: &[u8],
    data: Bytes,
    config: TracingInspectorConfig,
    limit: Option<usize>,
) -> (TracingInspector, ExecutionResult) {
    let mut evm = Context::mainnet().with_db(database(code, child)).build_mainnet_with_inspector(
        TracingInspector::new(config)
            .with_limits(TraceLimits::default().set_max_recorded_bytes(limit)),
    );
    let result = evm
        .inspect_tx(TxEnv {
            gas_limit: 1_000_000,
            kind: TransactTo::Call(TARGET),
            data,
            ..Default::default()
        })
        .unwrap();
    (evm.into_inspector(), result.result)
}

#[test]
fn exact_budget_succeeds_and_exhaustion_never_changes_execution() {
    let cases: &[(&[u8], &[u8])] = &[
        (BIG_CALL, &hex!("00")),
        (MANY_CALLS, &hex!("60205ff3")),
        (CREATE, &hex!("00")),
        // Memory writes, storage, logs, and output.
        (&hex!("60015f5260025f555f5450600160205fa160205ff3"), &hex!("00")),
    ];
    for &(code, child) in cases {
        let data = Bytes::from(vec![7; 32]);
        let config = TracingInspectorConfig::all();
        let (unlimited, expected) = run(code, child, data.clone(), config, None);
        assert!(expected.is_success());
        let total = unlimited.recorded_bytes();
        let (exact, actual) = run(code, child, data.clone(), config, Some(total));
        assert_eq!(actual, expected);
        assert_eq!(exact.traces().unwrap(), unlimited.traces().unwrap());
        // Exhaust at different points, including before the root, inside nested calls,
        // after memory expansion, and on the last retained output.
        for limit in [0, 1, total / 4, total / 2, total - 1] {
            let (mut limited, actual) = run(code, child, data.clone(), config, Some(limit));
            assert_eq!(actual, expected);
            assert!(limited.recorded_bytes() <= limit);
            let error = limited.check_limits().unwrap_err();
            assert!(matches!(error, TraceError::LimitExceeded { .. }));
            assert_eq!(limited.traces().unwrap_err(), error);
            assert_eq!(limited.traces_mut().unwrap_err(), error);
            assert_eq!(limited.geth_builder().unwrap_err(), error);
            assert_eq!(limited.clone().into_geth_builder().unwrap_err(), error);
            assert_eq!(limited.clone().into_parity_builder().unwrap_err(), error);
            assert_eq!(limited.clone().into_traces().unwrap_err(), error);
            // Neither changing config nor increasing the limit clears the error.
            limited.update_config(|_| TracingInspectorConfig::none());
            assert_eq!(
                limited.clone().with_limits(TraceLimits::default()).check_limits(),
                Err(error)
            );
            limited.fuse();
            assert_eq!(limited.recorded_bytes(), 0);
            assert_eq!(limited.limits().max_recorded_bytes, Some(limit));
            assert!(limited.check_limits().is_ok());
        }
    }
}

#[test]
fn inputs_outputs_and_metadata_are_charged_without_steps() {
    let config = TracingInspectorConfig::none();
    let (empty, _) = run(&hex!("00"), &[], Bytes::new(), config, None);
    let frame = core::mem::size_of::<CallTraceNode>() + core::mem::size_of::<usize>();
    assert_eq!(empty.recorded_bytes(), frame);
    let (ignored, _) = run(&hex!("00"), &[], Bytes::from(vec![7; 100]), config, None);
    assert_eq!(ignored.recorded_bytes(), frame);
    let (owned, _) =
        run(&hex!("00"), &[], Bytes::from(vec![7; 100]), config.set_record_inputs(true), None);
    assert_eq!(owned.recorded_bytes(), frame + 100);
    let (output, _) = run(&hex!("6104005ff3"), &[], Bytes::new(), config, None);
    assert_eq!(output.recorded_bytes(), frame + 1024);
    let (calls, _) = run(BIG_CALL, &hex!("00"), Bytes::new(), config.set_record_inputs(true), None);
    assert_eq!(
        calls.recorded_bytes(),
        2 * frame
            + core::mem::size_of::<usize>()
            + core::mem::size_of::<TraceMemberOrder>()
            + 65536
    );
    let (creation, _) = run(CREATE, &[], Bytes::new(), config.set_record_inputs(true), None);
    assert_eq!(
        creation.recorded_bytes(),
        2 * frame
            + core::mem::size_of::<usize>()
            + core::mem::size_of::<TraceMemberOrder>()
            + 10
            + 1
    );
}

#[test]
fn recorded_payloads_are_charged_independently() {
    let code = hex!("602a5f52600160205fa160025f555f545000");
    let config = TracingInspectorConfig::none().set_steps(true);
    let (baseline, _) = run(&code, &[], Bytes::new(), config, None);
    let (bytecode, _) = run(&code, &[], Bytes::new(), config.set_bytecode(true), None);
    assert_eq!(bytecode.recorded_bytes() - baseline.recorded_bytes(), code.len());
    let (logs, _) = run(&code, &[], Bytes::new(), config.set_record_logs(true), None);
    assert_eq!(
        logs.recorded_bytes() - baseline.recorded_bytes(),
        core::mem::size_of::<CallLog>() + core::mem::size_of::<TraceMemberOrder>() + 32 + 32
    );
    let (memory, _) = run(&code, &[], Bytes::new(), config.set_memory_snapshots(true), None);
    let recorded_memory: usize = memory.traces().unwrap().nodes()[0]
        .trace
        .steps
        .iter()
        .filter_map(|step| step.memory.as_ref())
        .map(|m| m.len())
        .sum();
    assert!(recorded_memory > 0);
    assert_eq!(memory.recorded_bytes() - baseline.recorded_bytes(), recorded_memory);
    let (deltas, _) = run(&code, &[], Bytes::new(), config.set_step_deltas(true), None);
    let recorded_deltas: usize = deltas.traces().unwrap().nodes()[0]
        .trace
        .step_deltas
        .iter()
        .map(|delta| {
            core::mem::size_of::<StepDelta>() + delta.memory.as_ref().map_or(0, |m| m.data.len())
        })
        .sum();
    assert!(recorded_deltas > 0);
    assert_eq!(deltas.recorded_bytes() - baseline.recorded_bytes(), recorded_deltas);
    let (immediates, _) = run(&code, &[], Bytes::new(), config.set_immediate_bytes(true), None);
    assert_eq!(immediates.recorded_bytes() - baseline.recorded_bytes(), 4);
    for mode in [
        StackSnapshotType::All,
        StackSnapshotType::Full,
        StackSnapshotType::Pushes,
        StackSnapshotType::Top,
    ] {
        let (stacks, _) = run(&code, &[], Bytes::new(), config.set_stack_snapshots(mode), None);
        let stack_bytes: usize = stacks.traces().unwrap().nodes()[0]
            .trace
            .steps
            .iter()
            .map(|step| {
                step.stack.as_ref().map_or(0, |s| core::mem::size_of_val(&**s))
                    + step.push_stack.as_ref().map_or(0, |s| core::mem::size_of_val(&**s))
            })
            .sum();
        assert!(stack_bytes > 0);
        assert_eq!(stacks.recorded_bytes() - baseline.recorded_bytes(), stack_bytes);
    }
    let (storage, _) = run(&code, &[], Bytes::new(), config.set_state_diffs(true), None);
    let changes = storage.traces().unwrap().nodes()[0]
        .trace
        .steps
        .iter()
        .filter(|step| step.storage_change.is_some())
        .count();
    assert!(changes > 0);
    assert_eq!(
        storage.recorded_bytes() - baseline.recorded_bytes(),
        changes * core::mem::size_of::<StorageChange>()
    );
    let (no_returns, _) = run(MANY_CALLS, &hex!("60205ff3"), Bytes::new(), config, None);
    let (returns, _) = run(
        MANY_CALLS,
        &hex!("60205ff3"),
        Bytes::new(),
        TracingInspectorConfig { record_returndata_snapshots: true, ..config },
        None,
    );
    let return_bytes: usize = returns
        .traces()
        .unwrap()
        .nodes()
        .iter()
        .flat_map(|node| &node.trace.steps)
        .map(|step| step.returndata.len())
        .sum();
    assert!(return_bytes > 0);
    assert_eq!(returns.recorded_bytes() - no_returns.recorded_bytes(), return_bytes);
    let step_count = baseline.traces().unwrap().nodes()[0].trace.steps.len();
    assert_eq!(
        baseline.recorded_bytes(),
        core::mem::size_of::<CallTraceNode>()
            + core::mem::size_of::<usize>()
            + step_count
                * (core::mem::size_of::<CallTraceStep>()
                    + core::mem::size_of::<TraceMemberOrder>())
    );
}

#[test]
fn debug_and_mux_return_errors_and_preserve_limits_across_transactions() {
    let mux: GethDebugTracingOptions = serde_json::from_value(serde_json::json!({
        "tracer": "muxTracer", "tracerConfig": { "callTracer": {}, "prestateTracer": {} }
    }))
    .unwrap();
    for opts in [GethDebugTracingOptions::call_tracer(CallConfig::default()), mux] {
        let mut inspector = DebugInspector::new(opts)
            .unwrap()
            .with_limits(TraceLimits::default().set_max_recorded_bytes(Some(1024)))
            .unwrap();
        for _ in 0..2 {
            let mut evm = Context::mainnet()
                .with_db(database(BIG_CALL, &hex!("00")))
                .build_mainnet_with_inspector(&mut inspector);
            let result = evm
                .inspect_tx(TxEnv {
                    gas_limit: 1_000_000,
                    kind: TransactTo::Call(TARGET),
                    ..Default::default()
                })
                .unwrap();
            assert!(result.result.is_success());
            let (ctx, inspector) = evm.ctx_inspector();
            let tx = ctx.tx().clone();
            let block = ctx.block().clone();
            let error = inspector.get_result(None, &tx, &block, &result, ctx.db_mut()).unwrap_err();
            assert!(matches!(
                error,
                DebugInspectorError::Trace(TraceError::LimitExceeded { .. })
                    | DebugInspectorError::MuxInspector(MuxError::Trace(
                        TraceError::LimitExceeded { .. }
                    ))
            ));
            inspector.fuse().unwrap();
        }
    }
}

#[test]
fn unsupported_tracers_do_not_silently_ignore_limits() {
    for json in [
        serde_json::json!({ "tracer": "4byteTracer" }),
        serde_json::json!({ "tracer": "muxTracer", "tracerConfig": { "4byteTracer": null, "callTracer": {} } }),
    ] {
        let opts = serde_json::from_value(json).unwrap();
        let error = DebugInspector::new(opts)
            .unwrap()
            .with_limits(TraceLimits::default().set_max_recorded_bytes(Some(1024)))
            .unwrap_err();
        assert_eq!(error, TraceError::UnsupportedTracer);
    }
}

#[test]
fn config_merging_and_late_limit_changes_cannot_reset_usage() {
    let (mut inspector, _) =
        run(&hex!("00"), &[], Bytes::new(), TracingInspectorConfig::none(), None);
    let used = inspector.recorded_bytes();
    inspector.config_mut().merge(TracingInspectorConfig::all());
    let inspector =
        inspector.with_limits(TraceLimits::default().set_max_recorded_bytes(Some(used - 1)));
    assert_eq!(inspector.recorded_bytes(), used);
    assert!(inspector.check_limits().is_err());
}
