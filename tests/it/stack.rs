//! Stack snapshot tests

use crate::utils::inspect_deploy_contract;
use alloy_primitives::{hex, Address, U256};
use revm::{
    database::CacheDB, database_interface::EmptyDB, primitives::hardfork::SpecId, Context,
    MainBuilder, MainContext,
};
use revm_inspectors::tracing::{StackSnapshotType, TracingInspector, TracingInspectorConfig};

/// Runs `PUSH1 1; PUSH1 2; PUSH1 3; STOP` as create init code and returns the recorded stack
/// snapshot of each step.
fn stack_snapshots(snapshot_type: StackSnapshotType) -> Vec<Option<Box<[U256]>>> {
    let mut insp = TracingInspector::new(
        TracingInspectorConfig::none().set_steps(true).set_stack_snapshots(snapshot_type),
    );

    let mut evm = Context::mainnet()
        .with_db(CacheDB::new(EmptyDB::default()))
        .build_mainnet()
        .with_inspector(&mut insp);

    let res = inspect_deploy_contract(
        &mut evm,
        hex!("60016002600300").into(),
        Address::ZERO,
        SpecId::CANCUN,
    );
    assert!(res.is_success());

    let nodes = insp.traces().nodes();
    assert_eq!(nodes.len(), 1);

    nodes[0].trace.steps.iter().map(|step| step.stack.clone()).collect()
}

#[test]
fn test_top_stack_snapshots() {
    let snapshots = stack_snapshots(StackSnapshotType::Top);

    assert_eq!(
        snapshots,
        vec![
            // The stack is recorded before each step executes.
            Some(Box::default()),
            Some(Box::from([U256::from(1)])),
            Some(Box::from([U256::from(2)])),
            Some(Box::from([U256::from(3)])),
        ]
    );
}

#[test]
fn test_top_stack_snapshots_match_the_top_of_full_snapshots() {
    let full = stack_snapshots(StackSnapshotType::Full);
    let top = stack_snapshots(StackSnapshotType::Top);

    assert_eq!(full.len(), top.len());
    for (full, top) in full.iter().zip(top) {
        let full_top: &[U256] =
            full.as_ref().unwrap().last().map(core::slice::from_ref).unwrap_or_default();
        assert_eq!(top.as_deref(), Some(full_top));
    }
}
