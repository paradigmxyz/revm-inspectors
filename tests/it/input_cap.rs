//! Bounds on the call input the tracer records.

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

/// Five `CALL`s of 1 KiB each, the shape a per-frame cap alone cannot bound.
///
/// `PUSH1 5 JUMPDEST` then a loop of `CALL` with `argsLength = 1024`, decrementing until zero.
const FIVE_SMALL_CALLS: &[u8] = &hex!("60055b5f5f6104005f5f604361fffff150600190038060025700");
const SMALL_CALL_INPUT_LEN: usize = 1024;

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
fn unlimited_by_default_records_the_whole_input() {
    let inspector = inspect(ONE_BIG_CALL, TracingInspectorConfig::none());
    let trace = &inspector.traces().nodes()[1].trace;
    assert_eq!(trace.data.len(), BIG_CALL_INPUT_LEN);
    assert_eq!(trace.full_data_len, None);
    assert!(!trace.is_input_truncated());
    assert!(!inspector.input_budget_exceeded());
    assert_eq!(inspector.recorded_input_bytes(), BIG_CALL_INPUT_LEN as u64);
}

#[test]
fn per_frame_cap_truncates_and_reports_the_true_length() {
    let inspector =
        inspect(ONE_BIG_CALL, TracingInspectorConfig::none().set_max_frame_input_bytes(Some(1024)));
    let trace = &inspector.traces().nodes()[1].trace;
    assert_eq!(trace.data.len(), 1024);
    assert_eq!(trace.full_data_len, Some(BIG_CALL_INPUT_LEN));
    assert!(trace.is_input_truncated());
    // the cap truncates by design, it does not make the response incomplete
    assert!(!inspector.input_budget_exceeded());
    assert_eq!(inspector.recorded_input_bytes(), 1024);
}

#[test]
fn budget_bounds_many_frames_each_under_the_cap() {
    // Five 1 KiB frames all fit under the per-frame cap, so only the budget bounds them.
    let inspector = inspect(
        FIVE_SMALL_CALLS,
        TracingInspectorConfig::none()
            .set_max_frame_input_bytes(Some(2048))
            .set_max_recorded_input_bytes(Some(3000)),
    );

    let nodes = inspector.traces().nodes();
    assert_eq!(nodes.len(), 6, "the call tree stays complete");
    assert_eq!(inspector.recorded_input_bytes(), 3000);
    assert!(inspector.input_budget_exceeded());

    let recorded: Vec<_> = nodes[1..].iter().map(|node| node.trace.data.len()).collect();
    assert_eq!(recorded, vec![1024, 1024, 952, 0, 0]);

    // the frames the budget clipped carry their true length, the ones it did not are unmarked
    let full_lens: Vec<_> = nodes[1..].iter().map(|node| node.trace.full_data_len).collect();
    let clipped = Some(SMALL_CALL_INPUT_LEN);
    assert_eq!(full_lens, vec![None, None, clipped, clipped, clipped]);
}

#[test]
fn budget_is_not_consumed_when_unlimited() {
    let inspector = inspect(FIVE_SMALL_CALLS, TracingInspectorConfig::none());
    assert_eq!(inspector.recorded_input_bytes(), 5 * SMALL_CALL_INPUT_LEN as u64);
    assert!(!inspector.input_budget_exceeded());
    assert!(inspector.traces().nodes()[1..].iter().all(|node| !node.trace.is_input_truncated()));
}

#[test]
fn fuse_resets_the_budget() {
    let mut inspector = inspect(
        ONE_BIG_CALL,
        TracingInspectorConfig::none().set_max_recorded_input_bytes(Some(1024)),
    );
    assert!(inspector.input_budget_exceeded());
    inspector.fuse();
    assert_eq!(inspector.recorded_input_bytes(), 0);
    assert!(!inspector.input_budget_exceeded());
}

#[test]
fn owned_calldata_is_capped_too() {
    // The root frame's input is a `CallInput::Bytes`, not a shared memory slice.
    let inspector = inspect_with_data(
        &hex!("00"),
        Bytes::from(vec![7u8; 100]),
        TracingInspectorConfig::none().set_max_frame_input_bytes(Some(4)),
    );
    let trace = &inspector.traces().nodes()[0].trace;
    assert_eq!(trace.data.as_ref(), &[7, 7, 7, 7]);
    assert_eq!(trace.full_data_len, Some(100));
    assert!(inspector.input_truncated());
}

#[test]
fn create_init_code_is_capped_and_charged() {
    // `PUSH2 0x0800 PUSH0 PUSH0 CREATE STOP`: 2 KiB of zeroed memory as init code.
    let inspector = inspect(
        &hex!("6108005f5ff000"),
        TracingInspectorConfig::none().set_max_frame_input_bytes(Some(512)),
    );
    let trace = &inspector.traces().nodes()[1].trace;
    assert!(trace.kind.is_any_create());
    assert_eq!(trace.data.len(), 512);
    assert_eq!(trace.full_data_len, Some(2048));
    assert_eq!(inspector.recorded_input_bytes(), 512);
}
