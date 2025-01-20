//! Edge coverage tests

use alloy_primitives::{hex, Address, U256};
use revm::{
    db::{CacheDB, EmptyDB},
    primitives::{
        BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, ExecutionResult, HandlerCfg,
        Output, SpecId, TransactTo, TxEnv,
    },
    DatabaseCommit,
};

use crate::utils::inspect;
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

    let mut db = CacheDB::new(EmptyDB::default());

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(SpecId::LONDON));

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create,
            data: code.into(),
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    // Create contract
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    let acc = db.load_account(deployer).unwrap();
    acc.info.balance = U256::from(u64::MAX);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        transact_to: TransactTo::Call(addr),
        // 'cast cd "Y(bool)" true'
        data: hex!("f42e8cdd0000000000000000000000000000000000000000000000000000000000000001")
            .into(),
        ..Default::default()
    };

    let mut insp = EdgeCovInspector::new();

    let env = EnvWithHandlerCfg::new_with_cfg_env(cfg.clone(), BlockEnv::default(), tx_env.clone());
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let mut counts: Vec<u8> = insp.hitcount.iter().map(|x| x.0).collect();
    // The `break` statement prevents the post-increment of the loop counter, so we expect 1 less
    // edge to be hit.
    assert_eq!(counts.iter().filter(|&x| *x != 0).count(), 11);
    assert_eq!(counts.iter().filter(|&x| *x == 1).count(), 11);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        transact_to: TransactTo::Call(addr),
        // 'cast cd "Y(bool)" false'
        data: hex!("f42e8cdd0000000000000000000000000000000000000000000000000000000000000000")
            .into(),
        ..Default::default()
    };
    let env = EnvWithHandlerCfg::new_with_cfg_env(cfg.clone(), BlockEnv::default(), tx_env.clone());
    insp.reset();
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    // There should be 12 non-zero counts and that two edges have been hit 255 times.
    counts = insp.hitcount.iter().map(|x| x.0).collect();
    counts.sort();
    assert_eq!(counts[counts.len() - 1], 255);
    assert_eq!(counts[counts.len() - 2], 255);
    assert_eq!(counts.iter().filter(|&x| *x != 0).count(), 12);
}
