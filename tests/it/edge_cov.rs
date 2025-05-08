//! Edge coverage tests

use alloy_primitives::{hex, Address, U256};
use revm::{
    context::TxEnv,
    context_interface::{
        result::{ExecutionResult, Output},
        ContextTr, TransactTo,
    },
    database::CacheDB,
    database_interface::EmptyDB,
    handler::EvmTr,
    inspector::InspectorEvmTr,
    primitives::hardfork::SpecId,
    Context, DatabaseCommit, ExecuteEvm, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::{
    edge_cov::EdgeCovInspector,
    tracing::{TracingInspector, TracingInspectorConfig},
};

#[test]
fn test_edge_coverage() {
    /*
    contract X {
        function Y(bool yes) external {
            for (uint256 i = 0; i < 255; i++) {
                if (yes) {
                    break;
                }
            }
        }
    }
    */

    let code = hex!("6080604052348015600f57600080fd5b5060b580601d6000396000f3fe6080604052348015600f57600080fd5b506004361060285760003560e01c8063f42e8cdd14602d575b600080fd5b603c60383660046058565b603e565b005b60005b60ff811015605457816054576001016041565b5050565b600060208284031215606957600080fd5b81358015158114607857600080fd5b939250505056fea2646970667358221220a206d90c473b6930258d5789495c41b79941b5334c47a76b6e618d3571716d5164736f6c634300081c0033");
    let deployer = Address::ZERO;

    let ctx = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::LONDON)
        .with_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Create,
            data: code.into(),
            ..Default::default()
        })
        .with_db(CacheDB::new(EmptyDB::default()));

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    let mut evm = ctx.build_mainnet_with_inspector(&mut insp);

    // Create contract
    let res = evm.inspect_replay().unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    evm.ctx().db().commit(res.state);

    let acc = evm.ctx().db().load_account(deployer).unwrap();
    acc.info.balance = U256::from(u64::MAX);

    let tx = TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        kind: TransactTo::Call(addr),
        nonce: 1,
        // 'cast cd "Y(bool)" true'
        data: hex!("f42e8cdd0000000000000000000000000000000000000000000000000000000000000001")
            .into(),
        ..Default::default()
    };

    let insp = EdgeCovInspector::new();
    let mut evm = evm.with_inspector(insp);
    evm.set_tx(tx);
    let res = evm.inspect_replay().unwrap();
    assert!(res.result.is_success());

    let counts = evm.inspector().get_hitcount();
    assert_eq!(counts.iter().filter(|&x| *x != 0).count(), 11);
    assert_eq!(counts.iter().filter(|&x| *x == 1).count(), 11);

    evm.inspector().reset();
    evm.set_tx(TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        kind: TransactTo::Call(addr),
        nonce: 1,
        // 'cast cd "Y(bool)" false'
        data: hex!("f42e8cdd0000000000000000000000000000000000000000000000000000000000000000")
            .into(),
        ..Default::default()
    });
    let res = evm.inspect_replay().unwrap();
    assert!(res.result.is_success());

    // There should be 13 non-zero counts and two edges that have been hit 255 times.
    let mut counts = evm.inspector.into_hitcount();

    counts.sort();
    assert_eq!(counts[counts.len() - 1], 255);
    assert_eq!(counts[counts.len() - 2], 255);
    assert_eq!(counts.iter().filter(|&x| *x != 0).count(), 13);
}
