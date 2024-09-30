//! Parity tests

use crate::utils::{inspect, print_traces};
use alloy_primitives::{address, hex, map::HashSet, Address, U256};
use alloy_rpc_types_eth::TransactionInfo;
use alloy_rpc_types_trace::parity::{Action, CallAction, CallType, SelfdestructAction, TraceType};
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

    assert_eq!(insp.traces().nodes().len(), 1);
    let node = &insp.traces().nodes()[0];
    assert!(node.is_selfdestruct(), "{node:#?}");
    assert_eq!(node.trace.address, contract_address);
    assert_eq!(node.trace.selfdestruct_address, Some(contract_address));
    assert_eq!(node.trace.selfdestruct_refund_target, Some(deployer));
    assert_eq!(node.trace.selfdestruct_transferred_value, Some(value));

    let traces =
        insp.into_parity_builder().into_localized_transaction_traces(TransactionInfo::default());

    assert_eq!(traces.len(), 2);
    assert_eq!(
        traces[1].trace.action,
        Action::Selfdestruct(SelfdestructAction {
            address: contract_address,
            refund_address: deployer,
            balance: value,
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

    let traces =
        insp.into_parity_builder().into_localized_transaction_traces(TransactionInfo::default());

    assert_eq!(traces.len(), 3);
    assert!(traces[1].trace.action.is_create());
    assert_eq!(traces[1].trace.trace_address, vec![0]);
    assert_eq!(traces[1].trace.subtraces, 1);
    assert!(traces[2].trace.action.is_selfdestruct());
    assert_eq!(traces[2].trace.trace_address, vec![0, 0]);
}

// Minimal example of <https://github.com/paradigmxyz/reth/issues/9124>, <https://etherscan.io/tx/0x4f3638c40c0a5aba96f409cb47603cd30ed8ef084a9cba89169812d20fc9a04f>
#[test]
fn test_parity_call_selfdestruct() {
    let caller = address!("500a229A1047D3D684210BF1b67A26eB2994794a");
    // let to = address!("1AB3b12861B1B8a497fD3248EdCb7d844E60C8f5");
    let balance = U256::from(50000000000000000u128);
    let input = hex!("43d726d6");

    let code = hex!("608080604052606b908160108239f3fe6004361015600c57600080fd5b6000803560e01c6343d726d614602157600080fd5b346032578060031936011260325733ff5b80fdfea2646970667358221220f393fc6be90126d52315ccd38ae6608ac4fd5bef4c59e119e280b2a2b149d0dc64736f6c63430008190033");

    let deployer = address!("341348115259a8bf69f1f50101c227fced83bac6");
    let value = U256::from(69);

    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(deployer, AccountInfo { balance: value, ..Default::default() });

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(SpecId::LONDON));
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
    let to = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    db.accounts.get_mut(&to).unwrap().info.balance = balance;

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg,
        BlockEnv::default(),
        TxEnv {
            caller,
            gas_limit: 100000000,
            transact_to: TransactTo::Call(to),
            data: input.to_vec().into(),
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    match &res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Call(_) => {}
            _ => panic!("call failed"),
        },
        err => panic!("Execution failed: {err:?}"),
    }
    db.commit(res.state);

    let traces = insp
        .into_parity_builder()
        .into_trace_results(&res.result, &HashSet::from_iter([TraceType::Trace]));
    assert_eq!(traces.trace.len(), 2);

    assert_eq!(
        traces.trace[0].action,
        Action::Call(CallAction {
            from: caller,
            call_type: CallType::Call,
            gas: traces.trace[0].action.as_call().unwrap().gas,
            input: input.into(),
            to,
            value: U256::ZERO,
        })
    );
    assert_eq!(
        traces.trace[1].action,
        Action::Selfdestruct(SelfdestructAction { address: to, refund_address: caller, balance })
    );
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

    let trace_types = HashSet::from_iter([TraceType::StateDiff]);
    let mut insp = TracingInspector::new(TracingInspectorConfig::from_parity_config(&trace_types));
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let mut full_trace = insp.into_parity_builder().into_trace_results(&res.result, &trace_types);

    let state_diff = full_trace.state_diff.as_mut().unwrap();
    populate_state_diff(state_diff, db, res.state.iter()).unwrap();

    assert!(!state_diff.contains_key(&to));
    assert!(state_diff.contains_key(&caller));
}

#[test]
fn test_parity_delegatecall_selfdestruct() {
    /*
    contract DelegateCall {
        constructor() payable {}
        function close(address target) public {
            (bool success,) = target.delegatecall(abi.encodeWithSignature("close()"));
            require(success, "Delegatecall failed");
        }
    }
    contract SelfDestructTarget {
        function close() public {
            selfdestruct(payable(msg.sender));
        }
    }
    */

    // DelegateCall contract bytecode
    let delegate_code = hex!("6080604052348015600e575f80fd5b506103158061001c5f395ff3fe608060405234801561000f575f80fd5b5060043610610029575f3560e01c8063c74073a11461002d575b5f80fd5b610047600480360381019061004291906101d4565b610049565b005b5f8173ffffffffffffffffffffffffffffffffffffffff166040516024016040516020818303038152906040527f43d726d6000000000000000000000000000000000000000000000000000000007bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff83818316178352505050506040516100f19190610251565b5f60405180830381855af49150503d805f8114610129576040519150601f19603f3d011682016040523d82523d5f602084013e61012e565b606091505b5050905080610172576040517f08c379a0000000000000000000000000000000000000000000000000000000008152600401610169906102c1565b60405180910390fd5b5050565b5f80fd5b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6101a38261017a565b9050919050565b6101b381610199565b81146101bd575f80fd5b50565b5f813590506101ce816101aa565b92915050565b5f602082840312156101e9576101e8610176565b5b5f6101f6848285016101c0565b91505092915050565b5f81519050919050565b5f81905092915050565b8281835e5f83830152505050565b5f61022b826101ff565b6102358185610209565b9350610245818560208601610213565b80840191505092915050565b5f61025c8284610221565b915081905092915050565b5f82825260208201905092915050565b7f44656c656761746563616c6c206661696c6564000000000000000000000000005f82015250565b5f6102ab601383610267565b91506102b682610277565b602082019050919050565b5f6020820190508181035f8301526102d88161029f565b905091905056fea2646970667358221220f6409a1a1bfa02cbcb4d9818e921686c97eed1566fbd60951a91d232035e046c64736f6c634300081a0033");

    // SelfDestructTarget contract bytecode
    let target_code = hex!("6080604052348015600e575f80fd5b50608180601a5f395ff3fe6080604052348015600e575f80fd5b50600436106026575f3560e01c806343d726d614602a575b5f80fd5b60306032565b005b3373ffffffffffffffffffffffffffffffffffffffff16fffea26469706673582212202ecd1d2f481d093cc2831fe0350ce1fe0bc42bc5cf34eb0a9e40a83b564eb59464736f6c634300081a0033");

    let deployer = address!("341348115259a8bf69f1f50101c227fced83bac6");
    let mut db = CacheDB::new(EmptyDB::default());

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(SpecId::CANCUN));

    // Deploy DelegateCall contract
    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create,
            data: delegate_code.into(),
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let delegate_addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    // Deploy SelfDestructTarget contract
    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create,
            data: target_code.into(),
            ..Default::default()
        },
    );

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());
    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let target_addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    // Prepare the input data for the close(address) function call
    let mut input_data = hex!("c74073a1").to_vec(); // keccak256("close(address)")[:4]
    input_data.extend_from_slice(&[0u8; 12]); // Pad with zeros
    input_data.extend_from_slice(target_addr.as_slice());

    // Call DelegateCall contract with SelfDestructTarget address
    let mut insp = TracingInspector::new(TracingInspectorConfig::default_parity());
    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg,
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Call(delegate_addr),
            data: input_data.into(),
            ..Default::default()
        },
    );

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let traces =
        insp.into_parity_builder().into_localized_transaction_traces(TransactionInfo::default());

    assert_eq!(traces.len(), 3);

    let trace0 = &traces[0].trace;
    assert!(trace0.action.is_call());
    assert_eq!(trace0.trace_address.len(), 0);
    assert_eq!(trace0.subtraces, 1);
    let action0 = trace0.action.as_call().unwrap();
    assert_eq!(action0.call_type, CallType::Call);
    assert_eq!(action0.from, deployer);
    assert_eq!(action0.to, delegate_addr);

    let trace1 = &traces[1].trace;
    assert!(trace1.action.is_call());
    assert_eq!(trace1.trace_address, vec![0]);
    assert_eq!(trace1.subtraces, 1);
    let action1 = trace1.action.as_call().unwrap();
    assert_eq!(action1.call_type, CallType::DelegateCall);
    assert_eq!(action1.from, delegate_addr);
    assert_eq!(action1.to, target_addr);

    let trace2 = &traces[2].trace;
    assert!(trace2.action.is_selfdestruct());
    assert_eq!(trace2.trace_address, vec![0, 0]);
    assert_eq!(trace2.subtraces, 0);
    let action2 = trace2.action.as_selfdestruct().unwrap();
    assert_eq!(action2.address, delegate_addr);
    assert_eq!(action2.refund_address, deployer);
}
