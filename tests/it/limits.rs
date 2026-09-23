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
use revm_inspectors::tracing::{TraceLimits, TracingInspector, TracingInspectorConfig};

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
    TracingInspector::new(config)
        .with_limits(TraceLimits::default().set_max_recorded_bytes(Some(bytes)))
}

#[test]
fn budgets_are_enforced_during_execution_and_accessors_stay_infallible() {
    let programs: &[&[u8]] = &[
        // A 64 KiB child input.
        &hex!("5f5f620100005f5f604361fffff100"),
        // Repeated calls with memory and return data.
        &hex!("60055b5f5f6104005f5f604361fffff150600190038060025760205ff3"),
        // Identity precompile: no interpreter initialization callback.
        &hex!("60205f6104005f5f600461fffff100"),
        // CREATE with initcode returning one byte of runtime code.
        &hex!("6960fe5f5360015ff300005f52600a60165ff000"),
        // Memory, storage, a log, and output.
        &hex!("60015f5260025f555f5450600160205fa160205ff3"),
    ];
    for &code in programs {
        for config in [TracingInspectorConfig::all(), TracingInspectorConfig::default_parity()] {
            let (baseline, expected) = run(code, TracingInspector::new(config));
            let expected = expected.unwrap();
            assert!(expected.is_success());
            let total = baseline.recorded_bytes();
            let (exact, result) = run(code, limited(config, total));
            assert_eq!(result.unwrap(), expected);
            assert_eq!(exact.traces(), baseline.traces());
            // Walk every recording boundary, including failures with a pending EVM action.
            let mut bytes = 0;
            while bytes < total {
                let (mut inspector, result) = run(code, limited(config, bytes));
                assert!(
                    matches!(result, Err(EVMError::Custom(ref message)) if message == "trace recorded byte limit exceeded")
                );
                assert!(inspector.recorded_bytes() > bytes);
                bytes = inspector.recorded_bytes();
                // Accessors/builders have their original signatures, even after an error.
                let _ = inspector.traces();
                let _ = inspector.traces_mut();
                let _ = inspector.geth_builder();
                let _ = inspector.clone().into_geth_builder();
                let _ = inspector.clone().into_parity_builder();
                let _ = inspector.clone().into_traces();
                inspector.fuse();
                assert_eq!(inspector.recorded_bytes(), 0);
                let (_, result) = run(code, inspector);
                assert!(
                    matches!(result, Err(EVMError::Custom(_))),
                    "reset must preserve the limit"
                );
            }
        }
    }
}

#[test]
fn only_new_byte_buffers_are_charged() {
    let config = TracingInspectorConfig::none();
    // Root input, output and bytecode are shared Bytes clones, not new byte buffers.
    let (inspector, result) =
        run(&hex!("6104005ff3"), limited(config.set_record_inputs(true).set_bytecode(true), 0));
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 0);

    let call = hex!("5f5f620100005f5f604361fffff100");
    let (inspector, result) = run(&call, limited(config.set_record_inputs(true), 65536));
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 65536);
    let (_, result) = run(&call, limited(config.set_record_inputs(true), 65535));
    assert!(matches!(result, Err(EVMError::Custom(_))));
    let (inspector, result) = run(&call, limited(config, 0));
    assert!(result.unwrap().is_success());
    assert_eq!(inspector.recorded_bytes(), 0);

    // One memory snapshot allocation, reused across subsequent steps.
    let code = hex!("60015f5260025060035000");
    let config = config.steps();
    for (config, bytes) in [
        (config.memory_snapshots(), 32),
        (config.set_step_deltas(true), 32),
        (config.record_immediate_bytes(), 3),
        (config.stack_snapshots(), 0),
    ] {
        let (inspector, result) = run(&code, limited(config, bytes));
        assert!(result.unwrap().is_success());
        assert_eq!(inspector.recorded_bytes(), bytes);
    }
}
