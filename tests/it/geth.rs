//! Geth tests

use crate::utils::inspect;
use alloy_primitives::{hex, Address, Bytes};
use alloy_rpc_trace_types::geth::CallConfig;
use revm::{
    db::{CacheDB, EmptyDB},
    interpreter::CreateScheme,
    primitives::{
        BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, ExecutionResult, HandlerCfg,
        Output, SpecId, TransactTo, TxEnv,
    },
    DatabaseCommit,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn test_geth_calltracer_logs() {
    /*
    contract LogTracing {
        event Log(address indexed addr, uint256 value);

        fallback() external payable {
            emit Log(msg.sender, msg.value);

            try this.nestedEmitWithFailure() {} catch {}
            this.nestedEmitWithSuccess();
        }

        function nestedEmitWithFailure() external {
            emit Log(msg.sender, 0);
            require(false, "nestedEmitWithFailure");
        }

        function nestedEmitWithSuccess() external {
            emit Log(msg.sender, 0);
        }
    }
    */

    let code = hex!("608060405234801561001057600080fd5b5061020e806100206000396000f3fe6080604052600436106100295760003560e01c80636ae1ad40146100fc5780638384a00214610111575b60405134815233907ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e9060200160405180910390a2306001600160a01b0316636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561009957600080fd5b505af19250505080156100aa575060015b50306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100e657600080fd5b505af11580156100fa573d6000803e3d6000fd5b005b34801561010857600080fd5b506100fa610126565b34801561011d57600080fd5b506100fa6101a0565b6040516000815233907ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e9060200160405180910390a260405162461bcd60e51b81526020600482015260156024820152746e6573746564456d6974576974684661696c75726560581b604482015260640160405180910390fd5b6040516000815233907ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e9060200160405180910390a256fea264697066735822122083e63cf29044353f3804bd72ac17c34f2a07658fd4ec9c28e88df22634c7885564736f6c63430008130033");
    let deployer = Address::ZERO;

    let mut db = CacheDB::new(EmptyDB::default());

    let cfg = CfgEnvWithHandlerCfg::new(CfgEnv::default(), HandlerCfg::new(SpecId::LONDON));

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

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    db.commit(res.state);

    let mut insp =
        TracingInspector::new(TracingInspectorConfig::default_geth().set_record_logs(true));

    let env = EnvWithHandlerCfg::new_with_cfg_env(
        cfg,
        BlockEnv::default(),
        TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Call(addr),
            data: Bytes::default(), // call fallback
            ..Default::default()
        },
    );

    let (res, _) = inspect(&mut db, env, &mut insp).unwrap();
    assert!(res.result.is_success());

    let call_frame = insp
        .with_transaction_gas_used(res.result.gas_used())
        .into_geth_builder()
        .geth_call_traces(CallConfig::default().with_log(), res.result.gas_used());

    // two subcalls
    assert_eq!(call_frame.calls.len(), 2);

    // top-level call emitted one log
    assert_eq!(call_frame.logs.len(), 1);

    // first call failed, no logs
    assert!(call_frame.calls[0].logs.is_empty());

    // second call succeeded, one log
    assert_eq!(call_frame.calls[1].logs.len(), 1);
}
