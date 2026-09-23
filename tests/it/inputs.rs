//! Recording inputs is optional and does not affect execution.

use alloy_primitives::{hex, Address, Bytes};
use revm::{
    bytecode::Bytecode, context::TxEnv, context_interface::TransactTo, database::CacheDB,
    database_interface::EmptyDB, state::AccountInfo, Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

/// A single `CALL` passing 64 KiB of memory as calldata, which revm hands to the inspector as a
/// zero-copy `CallInput::SharedBuffer`.
///
/// `PUSH0 PUSH0 PUSH3 0x010000 PUSH0 PUSH0 PUSH1 0x43 PUSH2 0xffff CALL STOP`
const ONE_BIG_CALL: &[u8] = &hex!("5f5f620100005f5f604361fffff100");
const BIG_CALL_INPUT_LEN: usize = 65536;

/// Runs `code` with the given config, calling into a `STOP`-only child at `0x43`.
fn inspect(code: &[u8], config: TracingInspectorConfig) -> TracingInspector {
    inspect_with_data(code, Bytes::new(), config)
}

/// As [`inspect`], with `data` as the transaction calldata, which reaches the inspector as an
/// owned `CallInput::Bytes` rather than a shared memory slice.
fn inspect_with_data(code: &[u8], data: Bytes, config: TracingInspectorConfig) -> TracingInspector {
    let target = Address::with_last_byte(0x42);
    let mut db = CacheDB::<EmptyDB>::default();
    for (address, code) in [(target, code), (Address::with_last_byte(0x43), &hex!("00")[..])] {
        db.insert_account_info(
            address,
            AccountInfo::default().with_code(Bytecode::new_raw(code.to_vec().into())),
        );
    }
    let mut evm =
        Context::mainnet().with_db(db).build_mainnet_with_inspector(TracingInspector::new(config));
    evm.inspect_tx(TxEnv {
        gas_limit: 1_000_000,
        kind: TransactTo::Call(target),
        data,
        ..Default::default()
    })
    .unwrap();
    evm.into_inspector()
}

#[test]
fn shared_and_owned_call_inputs_are_optional() {
    for record_inputs in [false, true] {
        let inspector = inspect_with_data(
            ONE_BIG_CALL,
            Bytes::from_static(&[1, 2, 3, 4]),
            TracingInspectorConfig::none().set_record_inputs(record_inputs),
        );
        let nodes = inspector.traces().unwrap().nodes();
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|node| node.trace.success));
        assert_eq!(nodes[0].trace.data.len(), if record_inputs { 4 } else { 0 });
        assert_eq!(nodes[1].trace.data.len(), if record_inputs { BIG_CALL_INPUT_LEN } else { 0 });
    }
}

#[test]
fn creation_input_is_optional() {
    // Store init code that returns one byte of runtime code, then CREATE with those 10 bytes.
    let code = hex!("6960fe5f5360015ff300005f52600a60165ff000");
    for record_inputs in [false, true] {
        let inspector =
            inspect(&code, TracingInspectorConfig::none().set_record_inputs(record_inputs));
        let nodes = inspector.traces().unwrap().nodes();
        assert_eq!(nodes.len(), 2);
        let trace = &nodes[1].trace;
        assert!(trace.kind.is_any_create());
        assert!(trace.success);
        assert_eq!(trace.output.as_ref(), &[0xfe]);
        assert_eq!(trace.data.len(), if record_inputs { 10 } else { 0 });
    }
}

#[test]
fn disabling_recording_preserves_calldata_execution() {
    // Return the first calldata word.
    let code = hex!("5f355f5260205ff3");
    let data = Bytes::from(vec![7; 32]);
    for record_inputs in [false, true] {
        let inspector = inspect_with_data(
            &code,
            data.clone(),
            TracingInspectorConfig::none().set_record_inputs(record_inputs),
        );
        let trace = &inspector.traces().unwrap().nodes()[0].trace;
        assert!(trace.success);
        assert_eq!(trace.output, data);
        assert_eq!(trace.data.len(), if record_inputs { 32 } else { 0 });
    }
}
