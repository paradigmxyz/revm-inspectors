//! Transfer tests

use crate::utils::deploy_contract;
use alloy_primitives::{hex, Address, Bytes, Log, U256};
use revm::{
    context::TxEnv,
    context_interface::{ContextTr, TransactTo},
    database::CacheDB,
    database_interface::EmptyDB,
    handler::EvmTr,
    inspector::InspectorEvmTr,
    primitives::hardfork::SpecId,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::transfer::{
    TransferInspector, TransferKind, TransferOperation, TRANSFER_LOG_EMITTER,
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

    // Create contract
    let mut evm = context.build_mainnet();
    let addr =
        deploy_contract(&mut evm, code.into(), deployer, SpecId::LONDON).created_address().unwrap();

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

#[test]
fn test_transfer_logs_discard_reverted_calls() {
    // Runtime: call the caller with CALLVALUE, then revert.
    let code = hex!("6013600c60003960136000f36000600060006000343361fffff160006000fd");
    let deployer = Address::ZERO;

    let db = CacheDB::new(EmptyDB::default());
    let context = Context::mainnet().with_db(db).modify_cfg_chained(|c| c.spec = SpecId::LONDON);

    let mut evm = context.build_mainnet();
    let addr =
        deploy_contract(&mut evm, code.into(), deployer, SpecId::LONDON).created_address().unwrap();

    let acc = evm.ctx().db_mut().load_account(deployer).unwrap();
    acc.info.balance = U256::from(u64::MAX);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Call(addr),
        value: U256::from(10),
        nonce: 0,
        ..Default::default()
    };

    let mut evm = evm.with_inspector(TransferInspector::new(false));
    let res = evm.inspect_tx(tx_env.clone().modify().nonce(1).build_fill()).unwrap();
    assert!(!res.result.is_success());
    assert_eq!(evm.inspector().transfers().len(), 2);
    assert_eq!(res.result.logs().len(), 0);

    let mut evm = evm.with_inspector(TransferInspector::new(false).with_logs(true));
    let res = evm.inspect_tx(tx_env.modify().nonce(1).build_fill()).unwrap();
    assert!(!res.result.is_success());
    assert_eq!(evm.inspector().transfers().len(), 0);
    assert_eq!(transfer_log_count(res.result.logs()), 0);
}

#[test]
fn test_transfer_logs_discard_reverted_creates() {
    // Initcode: revert immediately.
    let code = hex!("60006000fd");
    let deployer = Address::ZERO;

    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        deployer,
        AccountInfo { balance: U256::from(u64::MAX), ..Default::default() },
    );
    let context = Context::mainnet().with_db(db).modify_cfg_chained(|c| c.spec = SpecId::LONDON);

    let mut evm = context.build_mainnet_with_inspector(TransferInspector::new(false));

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Create,
        data: code.into(),
        value: U256::from(10),
        nonce: 0,
        ..Default::default()
    };

    let res = evm.inspect_tx(tx_env.clone()).unwrap();
    assert!(!res.result.is_success());
    assert_eq!(evm.inspector().transfers().len(), 1);
    assert_eq!(res.result.logs().len(), 0);

    let mut evm = evm.with_inspector(TransferInspector::new(false).with_logs(true));

    let res = evm.inspect_tx(tx_env).unwrap();
    assert!(!res.result.is_success());
    assert_eq!(evm.inspector().transfers().len(), 0);
    assert_eq!(transfer_log_count(res.result.logs()), 0);
}

#[test]
fn test_transfer_logs_discard_reverted_value_call_with_nested_zero_call() {
    let deployer = Address::ZERO;
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let target = deploy_contract(&mut evm, initcode([0x00]), deployer, SpecId::LONDON)
        .created_address()
        .unwrap();
    let caller = deploy_contract(
        &mut evm,
        initcode(call_target_runtime(target, 0, true)),
        deployer,
        SpecId::LONDON,
    )
    .created_address()
    .unwrap();

    evm.ctx().db_mut().load_account(deployer).unwrap().info.balance = U256::from(u64::MAX);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Call(caller),
        value: U256::from(10),
        nonce: 2,
        ..Default::default()
    };

    let mut evm = evm.with_inspector(TransferInspector::new(false).with_logs(true));
    let res = evm.inspect_tx(tx_env).unwrap();
    assert!(!res.result.is_success());
    assert!(evm.inspector().transfers().is_empty());
    assert_eq!(transfer_log_count(res.result.logs()), 0);
}

#[test]
fn test_transfer_logs_discard_reverted_nested_value_call() {
    let deployer = Address::ZERO;
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let target = deploy_contract(&mut evm, initcode([0x00]), deployer, SpecId::LONDON)
        .created_address()
        .unwrap();
    let caller = deploy_contract(
        &mut evm,
        initcode(call_target_runtime(target, 10, true)),
        deployer,
        SpecId::LONDON,
    )
    .created_address()
    .unwrap();

    evm.ctx().db_mut().load_account(caller).unwrap().info.balance = U256::from(10);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Call(caller),
        nonce: 2,
        ..Default::default()
    };

    let mut evm = evm.with_inspector(TransferInspector::new(false).with_logs(true));
    let res = evm.inspect_tx(tx_env).unwrap();
    assert!(!res.result.is_success());
    assert!(evm.inspector().transfers().is_empty());
    assert_eq!(transfer_log_count(res.result.logs()), 0);
}

#[test]
fn test_transfer_logs_keep_nested_value_call_on_success() {
    let deployer = Address::ZERO;
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let target = deploy_contract(&mut evm, initcode([0x00]), deployer, SpecId::LONDON)
        .created_address()
        .unwrap();
    let caller = deploy_contract(
        &mut evm,
        initcode(call_target_runtime(target, 10, false)),
        deployer,
        SpecId::LONDON,
    )
    .created_address()
    .unwrap();

    evm.ctx().db_mut().load_account(caller).unwrap().info.balance = U256::from(10);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Call(caller),
        nonce: 2,
        ..Default::default()
    };

    let mut evm = evm.with_inspector(TransferInspector::new(false).with_logs(true));
    let res = evm.inspect_tx(tx_env).unwrap();
    assert!(res.result.is_success());
    assert_eq!(
        evm.inspector().transfers(),
        &[TransferOperation {
            kind: TransferKind::Call,
            from: caller,
            to: target,
            value: U256::from(10),
        }]
    );
    assert_eq!(transfer_log_count(res.result.logs()), 1);
}

#[test]
fn test_transfer_logs_discard_reverted_nested_selfdestruct() {
    let deployer = Address::ZERO;
    let beneficiary = Address::repeat_byte(0x42);
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let mut selfdestruct_runtime = Vec::with_capacity(22);
    selfdestruct_runtime.push(0x73);
    selfdestruct_runtime.extend_from_slice(beneficiary.as_slice());
    selfdestruct_runtime.push(0xff);

    let target =
        deploy_contract(&mut evm, initcode(selfdestruct_runtime), deployer, SpecId::LONDON)
            .created_address()
            .unwrap();
    let caller = deploy_contract(
        &mut evm,
        initcode(call_target_runtime(target, 0, true)),
        deployer,
        SpecId::LONDON,
    )
    .created_address()
    .unwrap();

    evm.ctx().db_mut().load_account(target).unwrap().info.balance = U256::from(10);

    let tx_env = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Call(caller),
        nonce: 2,
        ..Default::default()
    };

    let mut evm = evm.with_inspector(TransferInspector::new(false).with_logs(true));
    let res = evm.inspect_tx(tx_env).unwrap();
    assert!(!res.result.is_success());
    assert!(evm.inspector().transfers().is_empty());
    assert_eq!(transfer_log_count(res.result.logs()), 0);
}

fn initcode(runtime: impl AsRef<[u8]>) -> Bytes {
    let runtime = runtime.as_ref();
    assert!(runtime.len() <= u8::MAX as usize);

    let len = runtime.len() as u8;
    let mut code = Vec::with_capacity(12 + runtime.len());
    code.extend_from_slice(&[0x60, len, 0x60, 0x0c, 0x60, 0x00, 0x39]);
    code.extend_from_slice(&[0x60, len, 0x60, 0x00, 0xf3]);
    code.extend_from_slice(runtime);
    code.into()
}

fn call_target_runtime(target: Address, value: u8, revert: bool) -> Bytes {
    let mut runtime = Vec::with_capacity(40);
    runtime.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x60, value]);
    runtime.push(0x73);
    runtime.extend_from_slice(target.as_slice());
    runtime.extend_from_slice(&[0x61, 0xff, 0xff, 0xf1, 0x50]);

    if revert {
        runtime.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0xfd]);
    } else {
        runtime.push(0x00);
    }

    runtime.into()
}

fn transfer_log_count(logs: &[Log]) -> usize {
    logs.iter().filter(|log| log.address == TRANSFER_LOG_EMITTER).count()
}
