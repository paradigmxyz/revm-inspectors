//! Parity tests

use crate::utils::inspect;
use alloy_primitives::{hex, Address};
use alloy_rpc_types::TransactionInfo;
use revm::{
    db::{CacheDB, EmptyDB},
    interpreter::CreateScheme,
    primitives::{
        BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, ExecutionResult, Output, SpecId,
        TransactTo, TxEnv,
    },
    DatabaseCommit,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn test_parity_selfdestruct() {
    /*
    contract DummySelfDestruct {
        function close() public {
            selfdestruct(payable(msg.sender));
        }
    }
    */

    // simple contract that selfdestructs when a function is called
    let code = hex!("6080604052348015600f57600080fd5b50606a80601d6000396000f3fe6080604052348015600f57600080fd5b506004361060285760003560e01c806343d726d614602d575b600080fd5b603233ff5b00fea2646970667358221220e52c8372ad24b20a6f7e4a13772dbc1d00f7eb5d0934ed6d635d3f5bf47dbc9364736f6c634300080d0033");

    let deployer = Address::ZERO;

    let mut db = CacheDB::new(EmptyDB::default());

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), SpecId::LONDON);

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create(CreateScheme::Create),
            data: code.into(),
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg,
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Call(addr),
            data: hex!("43d726d6").into(),
            ..Default::default()
        },
    );

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let traces = insp
        .with_transaction_gas_used(res.result.gas_used())
        .into_parity_builder()
        .into_localized_transaction_traces(TransactionInfo::default());

    assert_eq!(traces.len(), 2);
    assert!(traces[1].trace.action.is_selfdestruct())
}

// Minimal example of <https://etherscan.io/tx/0xd81725127173cf1095a722cbaec118052e2626ddb914d61967fb4bf117969be0>
#[test]
fn test_parity_constructor_selfdestruct() {
    // simple contract that selfdestructs when a function is called

    /*
    contract DummySelfDestruct {
        function close() public {
            new Noop();
        }
    }
    contract Noop {
        constructor() {
            selfdestruct(payable(msg.sender));
        }
    }
    */

    let code = hex!("6080604052348015600f57600080fd5b5060b48061001e6000396000f3fe6080604052348015600f57600080fd5b506004361060285760003560e01c806343d726d614602d575b600080fd5b60336035565b005b604051603f90605e565b604051809103906000f080158015605a573d6000803e3d6000fd5b5050565b60148061006b8339019056fe6080604052348015600f57600080fd5b5033fffea264697066735822122087fcd1ed364913e41107ea336facf7b7f5972695b3e3abcf55dbb2452e124ea964736f6c634300080d0033");

    let deployer = Address::ZERO;

    let mut db = CacheDB::new(EmptyDB::default());

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), SpecId::LONDON);

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create(CreateScheme::Create),
            data: code.into(),
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg,
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Call(addr),
            data: hex!("43d726d6").into(),
            ..Default::default()
        },
    );

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let traces = insp
        .with_transaction_gas_used(res.result.gas_used())
        .into_parity_builder()
        .into_localized_transaction_traces(TransactionInfo::default());

    assert_eq!(traces.len(), 3);
    assert!(traces[1].trace.action.is_create());
    assert_eq!(traces[1].trace.trace_address, vec![0]);
    assert_eq!(traces[1].trace.subtraces, 1);
    assert!(traces[2].trace.action.is_selfdestruct());
    assert_eq!(traces[2].trace.trace_address, vec![0, 0]);
}
