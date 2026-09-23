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
            for bytes in [0, 1, total / 4, total / 2, total - 1] {
                let (mut inspector, result) = run(code, limited(config, bytes));
                assert!(
                    matches!(result, Err(EVMError::Custom(ref message)) if message == "trace recorded byte limit exceeded")
                );
                assert!(inspector.recorded_bytes() <= bytes);
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
fn recording_configuration_controls_charges() {
    let config = TracingInspectorConfig::none();
    let (baseline, _) = run(&hex!("00"), TracingInspector::new(config));
    let (inputs, _) = run(&hex!("00"), TracingInspector::new(config.set_record_inputs(true)));
    assert_eq!(inputs.recorded_bytes(), baseline.recorded_bytes() + 32);
    let (output, _) = run(&hex!("6104005ff3"), TracingInspector::new(config));
    assert_eq!(output.recorded_bytes(), baseline.recorded_bytes() + 1024);
    let (bytecode, _) = run(&hex!("6104005ff3"), TracingInspector::new(config.set_bytecode(true)));
    assert_eq!(bytecode.recorded_bytes(), output.recorded_bytes() + hex!("6104005ff3").len());
    // A budget sufficient for metadata remains sufficient when input recording is off.
    let (_, result) = run(&hex!("00"), limited(config, baseline.recorded_bytes()));
    assert!(result.unwrap().is_success());
}
