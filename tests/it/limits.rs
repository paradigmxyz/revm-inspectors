//! Recording limits use revm's existing execution error channel.

use alloy_primitives::{hex, Address, Bytes};
use core::convert::Infallible;
use revm::{
    bytecode::Bytecode,
    context::TxEnv,
    context_interface::{
        result::{EVMError, ExecutionResult},
        TransactTo,
    },
    database::CacheDB,
    database_interface::EmptyDB,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{
    TraceLimitBehavior, TraceLimits, TracingInspector, TracingInspectorConfig,
};

fn run(
    code: &[u8],
    inspector: TracingInspector,
) -> (TracingInspector, Result<ExecutionResult, EVMError<Infallible>>) {
    let target = Address::with_last_byte(0x42);
    let mut db = CacheDB::<EmptyDB>::default();
    for (address, code) in [(target, code), (Address::with_last_byte(0x43), &hex!("60205ff3")[..])]
    {
        db.insert_account_info(
            address,
            AccountInfo::default().with_code(Bytecode::new_raw(code.to_vec().into())),
        );
    }
    let mut evm = Context::mainnet().with_db(db).build_mainnet_with_inspector(inspector);
    let result = evm
        .inspect_tx(TxEnv {
            gas_limit: 1_000_000,
            kind: TransactTo::Call(target),
            data: Bytes::from(vec![7; 32]),
            ..Default::default()
        })
        .map(|result| result.result);
    (evm.into_inspector(), result)
}

fn limited(config: TracingInspectorConfig, bytes: usize) -> TracingInspector {
    TracingInspector::new(config).with_limits(
        TraceLimits::default()
            .set_max_recorded_bytes(Some(bytes))
            .set_behavior(TraceLimitBehavior::Halt),
    )
}

#[test]
fn step_budget_aborts_at_recording_boundaries() {
    let config = TracingInspectorConfig::all();
    for code in [
        // Repeated CALLs with memory snapshots.
        &hex!("60055b5f5f6104005f5f604361fffff150600190038060025760205ff3")[..],
        // CREATE with initcode returning one byte of runtime code.
        &hex!("6960fe5f5360015ff300005f52600a60165ff000")[..],
    ] {
        let (baseline, expected) = run(code, TracingInspector::new(config));
        let expected = expected.unwrap();
        assert!(expected.is_success());
        let total = baseline.recorded_bytes();
        assert!(total > 0);
        let (exact, result) = run(code, limited(config, total));
        assert_eq!(result.unwrap(), expected);
        assert_eq!(exact.traces(), baseline.traces());

        for bytes in [0, total / 2, total - 1] {
            let (inspector, result) = run(code, limited(config, bytes));
            assert!(
                matches!(result, Err(EVMError::Custom(ref message)) if message == "trace recorded byte limit exceeded")
            );
            assert!(inspector.limit_exceeded());
            assert!(inspector.recorded_bytes() <= bytes);
        }
    }
}

#[test]
fn input_budget_survives_reset() {
    let config = TracingInspectorConfig::none().set_record_inputs(true);
    for code in [
        // 64 KiB input to a contract.
        hex!("5f5f620100005f5f604361fffff100"),
        // Same input to the identity precompile, which has no interpreter callback.
        hex!("5f5f620100005f5f600461fffff100"),
    ] {
        let (inspector, result) = run(&code, limited(config, 65536));
        assert!(result.unwrap().is_success());
        assert_eq!(inspector.recorded_bytes(), 65536);

        let (mut inspector, result) = run(&code, limited(config, 65535));
        assert!(matches!(result, Err(EVMError::Custom(_))));
        assert!(inspector.limit_exceeded());
        inspector.fuse();
        assert_eq!(inspector.recorded_bytes(), 0);
        assert!(!inspector.limit_exceeded());
        let (_, result) = run(&code, inspector);
        assert!(matches!(result, Err(EVMError::Custom(_))));

        let (inspector, result) = run(&code, limited(config.set_record_inputs(false), 0));
        assert!(result.unwrap().is_success());
        assert_eq!(inspector.recorded_bytes(), 0);
    }
}

#[test]
fn only_new_byte_buffers_are_charged() {
    let config = TracingInspectorConfig::none();
    // Root input and output are shared Bytes clones, not new byte buffers.
    let (inspector, result) = run(&hex!("6104005ff3"), limited(config.set_record_inputs(true), 0));
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 0);

    // One memory snapshot allocation, reused across subsequent steps.
    let code = hex!("60015f5260025060035000");
    let config = config.steps();
    for (config, bytes) in [
        (config.memory_snapshots(), 32),
        (config.record_immediate_bytes(), 3),
        (config.stack_snapshots(), 0),
    ] {
        let (inspector, result) = run(&code, limited(config, bytes));
        assert!(result.unwrap().is_success());
        assert_eq!(inspector.recorded_bytes(), bytes);
    }
}

#[test]
fn repeated_staticcalls_cannot_bypass_input_budget() {
    // Reuse the same 64 KiB slice for 128 zero-gas STATICCALLs to an empty address.
    // Memory expansion is paid once; each recorded input still requires a fresh copy.
    // Regression: https://github.com/paradigmxyz/revm-inspectors/pull/518#issuecomment-5796099703
    let code = hex!("5f5f620100005f60445ffa50").repeat(128);
    let config = TracingInspectorConfig::from_geth_call_config(&Default::default());
    let (baseline, result) = run(&code, TracingInspector::new(config));
    assert!(result.unwrap().is_success());
    assert_eq!(baseline.recorded_bytes(), 128 * 65536);
    assert_eq!(baseline.traces().nodes().len(), 129);

    let (inspector, result) = run(&code, limited(config, 4 * 65536));
    assert!(
        matches!(result, Err(EVMError::Custom(ref message)) if message == "trace recorded byte limit exceeded")
    );
    // The fifth input is refused before copying, even without a child interpreter.
    assert_eq!(inspector.recorded_bytes(), 4 * 65536);
    assert_eq!(inspector.traces().nodes().len(), 6);
    assert!(inspector.limit_exceeded());

    let (inspector, result) = run(
        &code,
        TracingInspector::new(config)
            .with_limits(TraceLimits::default().set_max_recorded_bytes(Some(4 * 65536))),
    );
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 4 * 65536);
    assert!(inspector.limit_exceeded());
    let nodes = inspector.traces().nodes();
    assert_eq!(nodes.len(), 129);
    assert!(nodes[1..5].iter().all(|node| node.trace.data.len() == 65536));
    assert!(nodes[5..].iter().all(|node| node.trace.data.is_empty()));
    let call_trace = inspector.geth_builder().geth_call_traces(Default::default(), 0);
    assert_eq!(call_trace.calls.len(), 128);
    assert!(call_trace.calls[4..].iter().all(|call| call.input.is_empty()));

    let (inspector, result) = run(&code, limited(config.set_record_inputs(false), 0));
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 0);
    assert_eq!(inspector.traces().nodes().len(), 129);
}

#[test]
fn skip_is_default_and_stops_step_buffer_allocations() {
    assert_eq!(TraceLimitBehavior::default(), TraceLimitBehavior::Skip);
    let code = hex!("60015f5260025060035000");
    let config = TracingInspectorConfig::all();
    let (mut inspector, result) = run(
        &code,
        TracingInspector::new(config)
            .with_limits(TraceLimits::default().set_max_recorded_bytes(Some(0))),
    );
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 0);
    assert!(inspector.limit_exceeded());
    inspector.fuse();
    assert!(!inspector.limit_exceeded());
    let (inspector, result) = run(&code, inspector);
    assert!(result.unwrap().is_success());
    assert!(inspector.limit_exceeded());
}
