//! Geth tests

use crate::utils::{deploy_contract, inspect};
use alloy_primitives::{hex, map::HashMap, Address, Bytes};
use alloy_rpc_types_eth::TransactionInfo;
use alloy_rpc_types_trace::geth::{
    mux::MuxConfig, CallConfig, FlatCallConfig, GethDebugBuiltInTracerType, GethDebugTracerConfig,
    GethTrace, PreStateConfig, PreStateFrame,
};
use revm::{
    db::{CacheDB, EmptyDB},
    primitives::{
        BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, HandlerCfg, SpecId, TransactTo,
        TxEnv,
    },
};
use revm_inspectors::tracing::{MuxInspector, TracingInspector, TracingInspectorConfig};

#[test]
fn test_geth_calltracer_logs() {
    /*
    contract LogTracing {
        event Log(address indexed addr, uint256 value);

        fallback() external payable {
            emit Log(msg.sender, msg.value);

            try this.nestedEmitWithFailure() {} catch {}
            try this.nestedEmitWithFailureAfterNestedEmit() {} catch {}
            this.nestedEmitWithSuccess();
        }

        function nestedEmitWithFailure() external {
            emit Log(msg.sender, 0);
            require(false, "nestedEmitWithFailure");
        }

        function nestedEmitWithFailureAfterNestedEmit() external {
            this.doubleNestedEmitWithSuccess();
            require(false, "nestedEmitWithFailureAfterNestedEmit");
        }

        function doubleNestedEmitWithSuccess() external {
            emit Log(msg.sender, 0);
            this.nestedEmitWithSuccess();
        }

        function nestedEmitWithSuccess() external {
            emit Log(msg.sender, 0);
        }
    }
    */

    let code = hex!("608060405234801561001057600080fd5b506103ac806100206000396000f3fe60806040526004361061003f5760003560e01c80630332ed131461014d5780636ae1ad40146101625780638384a00214610177578063de7eb4f31461018c575b60405134815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561009d57600080fd5b505af19250505080156100ae575060015b50306001600160a01b0316630332ed136040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100ea57600080fd5b505af19250505080156100fb575060015b50306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561013757600080fd5b505af115801561014b573d6000803e3d6000fd5b005b34801561015957600080fd5b5061014b6101a1565b34801561016e57600080fd5b5061014b610253565b34801561018357600080fd5b5061014b6102b7565b34801561019857600080fd5b5061014b6102dd565b306001600160a01b031663de7eb4f36040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156101dc57600080fd5b505af11580156101f0573d6000803e3d6000fd5b505060405162461bcd60e51b8152602060048201526024808201527f6e6573746564456d6974576974684661696c75726541667465724e6573746564604482015263115b5a5d60e21b6064820152608401915061024a9050565b60405180910390fd5b6040516000815233906000805160206103578339815191529060200160405180910390a260405162461bcd60e51b81526020600482015260156024820152746e6573746564456d6974576974684661696c75726560581b604482015260640161024a565b6040516000815233906000805160206103578339815191529060200160405180910390a2565b6040516000815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561033c57600080fd5b505af1158015610350573d6000803e3d6000fd5b5050505056fef950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23ea2646970667358221220090a696b9fbd22c7d1cc2a0b6d4a48c32d3ba892480713689a3145b73cfeb02164736f6c63430008130033");
    let deployer = Address::ZERO;
    let (addr, mut evm) = deploy_contract(code.into(), deployer, SpecId::LONDON);

    let mut insp =
        TracingInspector::new(TracingInspectorConfig::default_geth().set_record_logs(true));

    let env = evm.env_with_tx(TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        transact_to: TransactTo::Call(addr),
        data: Bytes::default(), // call fallback
        ..Default::default()
    });

    let (res, _) = inspect(&mut evm.db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let call_frame = insp
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_call_traces(CallConfig::default().with_log(), res.result.gas_used());

    // top-level call succeeded, no log and three subcalls
    assert_eq!(call_frame.calls.len(), 3);
    assert_eq!(call_frame.logs.len(), 1);
    assert!(call_frame.error.is_none());

    // first subcall failed, and no logs
    assert!(call_frame.calls[0].logs.is_empty());
    assert!(call_frame.calls[0].error.is_some());

    // second subcall failed, with a two nested subcalls that emitted logs, but none should be
    // included
    assert_eq!(call_frame.calls[1].calls.len(), 1);
    assert!(call_frame.calls[1].logs.is_empty());
    assert!(call_frame.calls[1].error.is_some());
    assert!(call_frame.calls[1].calls[0].logs.is_empty());
    assert!(call_frame.calls[1].calls[0].error.is_none());
    assert!(call_frame.calls[1].calls[0].calls[0].logs.is_empty());
    assert!(call_frame.calls[1].calls[0].calls[0].error.is_none());

    // third subcall succeeded, one log
    assert_eq!(call_frame.calls[2].logs.len(), 1);
    assert!(call_frame.calls[2].error.is_none());
}

#[test]
fn test_geth_mux_tracer() {
    /*
    contract LogTracing {
        event Log(address indexed addr, uint256 value);

        fallback() external payable {
            emit Log(msg.sender, msg.value);

            try this.nestedEmitWithFailure() {} catch {}
            try this.nestedEmitWithFailureAfterNestedEmit() {} catch {}
            this.nestedEmitWithSuccess();
        }

        function nestedEmitWithFailure() external {
            emit Log(msg.sender, 0);
            require(false, "nestedEmitWithFailure");
        }

        function nestedEmitWithFailureAfterNestedEmit() external {
            this.doubleNestedEmitWithSuccess();
            require(false, "nestedEmitWithFailureAfterNestedEmit");
        }

        function doubleNestedEmitWithSuccess() external {
            emit Log(msg.sender, 0);
            this.nestedEmitWithSuccess();
        }

        function nestedEmitWithSuccess() external {
            emit Log(msg.sender, 0);
        }
    }
    */

    let code = hex!("608060405234801561001057600080fd5b506103ac806100206000396000f3fe60806040526004361061003f5760003560e01c80630332ed131461014d5780636ae1ad40146101625780638384a00214610177578063de7eb4f31461018c575b60405134815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561009d57600080fd5b505af19250505080156100ae575060015b50306001600160a01b0316630332ed136040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100ea57600080fd5b505af19250505080156100fb575060015b50306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561013757600080fd5b505af115801561014b573d6000803e3d6000fd5b005b34801561015957600080fd5b5061014b6101a1565b34801561016e57600080fd5b5061014b610253565b34801561018357600080fd5b5061014b6102b7565b34801561019857600080fd5b5061014b6102dd565b306001600160a01b031663de7eb4f36040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156101dc57600080fd5b505af11580156101f0573d6000803e3d6000fd5b505060405162461bcd60e51b8152602060048201526024808201527f6e6573746564456d6974576974684661696c75726541667465724e6573746564604482015263115b5a5d60e21b6064820152608401915061024a9050565b60405180910390fd5b6040516000815233906000805160206103578339815191529060200160405180910390a260405162461bcd60e51b81526020600482015260156024820152746e6573746564456d6974576974684661696c75726560581b604482015260640161024a565b6040516000815233906000805160206103578339815191529060200160405180910390a2565b6040516000815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561033c57600080fd5b505af1158015610350573d6000803e3d6000fd5b5050505056fef950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23ea2646970667358221220090a696b9fbd22c7d1cc2a0b6d4a48c32d3ba892480713689a3145b73cfeb02164736f6c63430008130033");
    let deployer = Address::ZERO;
    let (addr, mut evm) = deploy_contract(code.into(), deployer, SpecId::LONDON);

    let call_config = CallConfig { only_top_call: Some(false), with_log: Some(true) };
    let flatcall_config =
        FlatCallConfig { convert_parity_errors: Some(true), include_precompiles: None };
    let prestate_config = PreStateConfig { diff_mode: Some(false), ..Default::default() };

    let config = MuxConfig(HashMap::from_iter([
        (GethDebugBuiltInTracerType::FourByteTracer, None),
        (
            GethDebugBuiltInTracerType::CallTracer,
            Some(GethDebugTracerConfig(serde_json::to_value(call_config).unwrap())),
        ),
        (
            GethDebugBuiltInTracerType::PreStateTracer,
            Some(GethDebugTracerConfig(serde_json::to_value(prestate_config).unwrap())),
        ),
        (
            GethDebugBuiltInTracerType::FlatCallTracer,
            Some(GethDebugTracerConfig(serde_json::to_value(flatcall_config).unwrap())),
        ),
    ]));

    let mut insp = MuxInspector::try_from_config(config.clone()).unwrap();

    let env = evm.env_with_tx(TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        transact_to: TransactTo::Call(addr),
        data: Bytes::default(), // call fallback
        ..Default::default()
    });

    let (res, _) = inspect(&mut evm.db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let frame = insp.try_into_mux_frame(&res, &evm.db, TransactionInfo::default()).unwrap();

    assert_eq!(frame.0.len(), 4);
    assert!(frame.0.contains_key(&GethDebugBuiltInTracerType::FourByteTracer));
    assert!(frame.0.contains_key(&GethDebugBuiltInTracerType::CallTracer));
    assert!(frame.0.contains_key(&GethDebugBuiltInTracerType::PreStateTracer));
    assert!(frame.0.contains_key(&GethDebugBuiltInTracerType::FlatCallTracer));

    let four_byte_frame = frame.0[&GethDebugBuiltInTracerType::FourByteTracer].clone();
    match four_byte_frame {
        GethTrace::FourByteTracer(four_byte_frame) => {
            assert_eq!(four_byte_frame.0.len(), 4);
            assert!(four_byte_frame.0.contains_key("0x0332ed13-0"));
            assert!(four_byte_frame.0.contains_key("0x6ae1ad40-0"));
            assert!(four_byte_frame.0.contains_key("0x8384a002-0"));
            assert!(four_byte_frame.0.contains_key("0xde7eb4f3-0"));
        }
        _ => panic!("Expected FourByteTracer"),
    }

    let call_frame = frame.0[&GethDebugBuiltInTracerType::CallTracer].clone();
    match call_frame {
        GethTrace::CallTracer(call_frame) => {
            assert_eq!(call_frame.calls.len(), 3);
            assert_eq!(call_frame.logs.len(), 1);
        }
        _ => panic!("Expected CallTracer"),
    }

    let prestate_frame = frame.0[&GethDebugBuiltInTracerType::PreStateTracer].clone();
    match prestate_frame {
        GethTrace::PreStateTracer(prestate_frame) => {
            if let PreStateFrame::Default(prestate_mode) = prestate_frame {
                assert_eq!(prestate_mode.0.len(), 2);
            } else {
                panic!("Expected Default PreStateFrame");
            }
        }
        _ => panic!("Expected PreStateTracer"),
    }

    let flatcall_frame = frame.0[&GethDebugBuiltInTracerType::FlatCallTracer].clone();
    match flatcall_frame {
        GethTrace::FlatCallTracer(traces) => {
            assert_eq!(traces.len(), 6);
            assert!(traces[0].trace.error.is_none());
            assert!(traces[1].trace.error.is_some());
            assert!(traces[2].trace.error.is_some());
            assert!(traces[3].trace.error.is_none());
            assert!(traces[4].trace.error.is_none());
            assert!(traces[5].trace.error.is_none());
        }
        _ => panic!("Expected FlatCallTracer"),
    }
}

#[test]
fn test_geth_inspector_reset() {
    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    let mut db = CacheDB::new(EmptyDB::default());
    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(SpecId::LONDON));
    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg.clone(),
        BlockEnv::default(),
        TxEnv {
            caller: Address::ZERO,
            gas_limit: 1000000,
            gas_price: Default::default(),
            transact_to: TransactTo::Call(Address::ZERO),
            ..Default::default()
        },
    );

    assert_eq!(insp.traces().nodes().first().unwrap().trace.gas_limit, 0);

    // first run inspector
    let (res, env) = inspect(&mut db, env.clone(), &mut insp).unwrap();
    assert!(res.result.is_success());
    assert_eq!(
        insp.clone()
            .with_transaction_gas_limit(env.tx.gas_limit)
            .traces()
            .nodes()
            .first()
            .unwrap()
            .trace
            .gas_limit,
        1000000
    );

    // reset the inspector
    insp.fuse();
    assert_eq!(insp.traces().nodes().first().unwrap().trace.gas_limit, 0);

    // second run inspector after reset
    let (res, env) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());
    assert_eq!(
        insp.with_transaction_gas_limit(env.tx.gas_limit)
            .traces()
            .nodes()
            .first()
            .unwrap()
            .trace
            .gas_limit,
        1000000
    );
}
