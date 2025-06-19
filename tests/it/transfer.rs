//! Transfer tests

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
    Context, DatabaseCommit, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::{
    tracing::{TracingInspector, TracingInspectorConfig},
    transfer::{TransferInspector, TransferKind, TransferOperation},
};

#[test]
fn test_internal_transfers() {
    /*
    contract Transfer {

        function sendViaCall(address payable _to) public payable {
            (bool sent, bytes memory data) = _to.call{value: msg.value}("");
        }
    }
    */

    let code = hex!("608060405234801561001057600080fd5b5060ef8061001f6000396000f3fe608060405260043610601c5760003560e01c8063830c29ae146021575b600080fd5b6030602c366004608b565b6032565b005b600080826001600160a01b03163460405160006040518083038185875af1925050503d8060008114607e576040519150601f19603f3d011682016040523d82523d6000602084013e6083565b606091505b505050505050565b600060208284031215609c57600080fd5b81356001600160a01b038116811460b257600080fd5b939250505056fea26469706673582212201654bdbf09c088897c9b02f3ba9df280b136ef99c3a05ca5a21d9a10fd912d3364736f6c634300080d0033");
    let deployer = Address::ZERO;

    let db = CacheDB::new(EmptyDB::default());

    let context = Context::mainnet().with_db(db).modify_cfg_chained(|c| c.spec = SpecId::LONDON);

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    // Create contract
    let mut evm = context.build_mainnet_with_inspector(&mut insp);
    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Create,
            data: code.into(),
            ..Default::default()
        })
        .unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    evm.ctx().db_mut().commit(res.state);

    let acc = evm.ctx().db_mut().load_account(deployer).unwrap();
    acc.info.balance = U256::from(u64::MAX);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        kind: TransactTo::Call(addr),
        data: hex!("830c29ae0000000000000000000000000000000000000000000000000000000000000000")
            .into(),
        value: U256::from(10),
        nonce: 0,
        ..Default::default()
    };

    let mut evm = evm.with_inspector(TransferInspector::new(false));

    let res = evm.inspect_tx(tx_env.clone().modify().nonce(1).build_fill()).unwrap();
    assert!(res.result.is_success());

    assert_eq!(evm.inspector().transfers().len(), 2);
    assert_eq!(
        evm.inspector().transfers()[0],
        TransferOperation {
            kind: TransferKind::Call,
            from: deployer,
            to: addr,
            value: U256::from(10),
        }
    );
    assert_eq!(
        evm.inspector().transfers()[1],
        TransferOperation {
            kind: TransferKind::Call,
            from: addr,
            to: deployer,
            value: U256::from(10),
        }
    );

    let mut evm = evm.with_inspector(TransferInspector::internal_only());
    let res = evm.inspect_tx(tx_env.clone().modify().nonce(1).build_fill()).unwrap();
    assert!(res.result.is_success());

    assert_eq!(evm.inspector().transfers().len(), 1);
    assert_eq!(
        evm.inspector().transfers()[0],
        TransferOperation {
            kind: TransferKind::Call,
            from: addr,
            to: deployer,
            value: U256::from(10),
        }
    );
}
