//! Parity tests

use crate::utils::{inspect, print_traces};
use alloy_primitives::{address, hex, Address, U256};
use alloy_rpc_types::{
    trace::parity::{Action, SelfdestructAction, TraceType},
    TransactionInfo,
};
use revm::{
    db::{CacheDB, EmptyDB},
    primitives::{
        AccountInfo, BlobExcessGasAndPrice, BlockEnv, CfgEnv, CfgEnvWithHandlerCfg,
        EnvWithHandlerCfg, ExecutionResult, HandlerCfg, Output, SpecId, TransactTo, TxEnv,
    },
    DatabaseCommit,
};
use revm_inspectors::tracing::{
    parity::populate_state_diff, TracingInspector, TracingInspectorConfig,
};
use std::collections::HashSet;

#[test]
fn test_parity_selfdestruct_london() {
    test_parity_selfdestruct(SpecId::LONDON);
}

#[test]
fn test_parity_selfdestruct_cancun() {
    test_parity_selfdestruct(SpecId::CANCUN);
}

fn test_parity_selfdestruct(spec_id: SpecId) {
    /*
    contract DummySelfDestruct {
        constructor() payable {}
        function close() public {
            selfdestruct(payable(msg.sender));
        }
    }
    */

    // simple contract that selfdestructs when a function is called
    let code = hex!("608080604052606b908160108239f3fe6004361015600c57600080fd5b6000803560e01c6343d726d614602157600080fd5b346032578060031936011260325733ff5b80fdfea2646970667358221220f393fc6be90126d52315ccd38ae6608ac4fd5bef4c59e119e280b2a2b149d0dc64736f6c63430008190033");

    let deployer = address!("341348115259a8bf69f1f50101c227fced83bac6");
    let value = U256::from(69);

    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(deployer, AccountInfo { balance: value, ..Default::default() });

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(spec_id));
    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create,
            data: code.into(),
            value,
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let contract_address = match res.result {
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
            transact_to: TransactTo::Call(contract_address),
            data: hex!("43d726d6").into(),
            ..Default::default()
        },
    );

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success(), "{res:#?}");

    // TODO: Transfer still happens in Cancun, but this is not reflected in the trace.
    let (expected_value, expected_target) =
        if spec_id < SpecId::CANCUN { (value, Some(deployer)) } else { (U256::ZERO, None) };

    {
        assert_eq!(insp.get_traces().nodes().len(), 1);
        let node = &insp.get_traces().nodes()[0];
        assert!(node.is_selfdestruct(), "{node:#?}");
        assert_eq!(node.trace.address, contract_address);
        assert_eq!(node.trace.selfdestruct_refund_target, expected_target);
        assert_eq!(node.trace.value, expected_value);
    }

    let traces = insp
        .with_transaction_gas_used(res.result.gas_used())
        .into_parity_builder()
        .into_localized_transaction_traces(TransactionInfo::default());

    assert_eq!(traces.len(), 2);
    assert_eq!(
        traces[1].trace.action,
        Action::Selfdestruct(SelfdestructAction {
            address: contract_address,
            refund_address: expected_target.unwrap_or_default(),
            balance: expected_value,
        })
    );
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
    print_traces(&insp);

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
    print_traces(&insp);

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

// Minimal example of <https://github.com/paradigmxyz/reth/issues/8610>
// <https://sepolia.etherscan.io/tx/0x19dc9c21232699a274849fac7443be6de819755a07b7175a21d337e223070709>
#[test]
fn test_parity_statediff_blob_commit() {
    let caller = address!("283b5b7d75e3e6b84b8e2161e8a468d733bbbe8d");

    let mut db = CacheDB::new(EmptyDB::default());

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(SpecId::CANCUN));

    db.insert_account_info(
        caller,
        AccountInfo { balance: U256::from(u64::MAX), ..Default::default() },
    );

    let to = address!("15dd773dad3f630773a0e771e9b221f4c8b9b939");

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv {
            basefee: U256::from(100),
            blob_excess_gas_and_price: Some(BlobExcessGasAndPrice::new(100)),
            ..Default::default()
        },
        TxEnv {
            caller,
            gas_limit: 1000000,
            transact_to: TransactTo::Call(to),
            gas_price: U256::from(150),
            blob_hashes: vec!["0x01af2fd94f17364bc8ef371c4c90c3a33855ff972d10b9c03d0445b3fca063ea"
                .parse()
                .unwrap()],
            max_fee_per_blob_gas: Some(U256::from(1000000000)),
            ..Default::default()
        },
    );

    let trace_types = HashSet::from([TraceType::StateDiff]);
    let mut insp = TracingInspector::new(TracingInspectorConfig::from_parity_config(&trace_types));
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let mut full_trace = insp.into_parity_builder().into_trace_results(&res.result, &trace_types);

    let state_diff = full_trace.state_diff.as_mut().unwrap();
    populate_state_diff(state_diff, db, res.state.iter()).unwrap();

    assert!(!state_diff.contains_key(&to));
    assert!(state_diff.contains_key(&caller));
}
