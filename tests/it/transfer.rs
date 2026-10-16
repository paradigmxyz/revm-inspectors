//! Transfer tests

use super::utils::EvmDb;
use alloy_primitives::{hex, Address, Bytes, Log, B256, U256};
use revm::{
    bytecode::Bytecode,
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
    state::AccountInfo,
    Context, DatabaseCommit, InspectCommitEvm, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::{
    tracing::{TracingInspector, TracingInspectorConfig},
    transfer::{
        TransferInspector, TransferKind, TransferOperation, TRANSFER_EVENT_TOPIC,
        TRANSFER_LOG_EMITTER,
    },
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

const SENDER: Address = Address::with_last_byte(0x41);
const RECIPIENT: Address = Address::with_last_byte(0x42);
const REVERTING: Address = Address::with_last_byte(0x43);
const RECEIVER: Address = Address::with_last_byte(0x44);
const CALLER: Address = Address::with_last_byte(0x45);

fn transfer_evm(
    inspector: TransferInspector,
    accounts: impl IntoIterator<Item = (Address, Bytes, u64)>,
) -> EvmDb<CacheDB<EmptyDB>, TransferInspector> {
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        SENDER,
        AccountInfo { balance: U256::from(1_000_000), ..Default::default() },
    );
    for (address, code, balance) in accounts {
        db.insert_account_info(
            address,
            AccountInfo {
                balance: U256::from(balance),
                code: Some(Bytecode::new_raw(code)),
                ..Default::default()
            },
        );
    }
    Context::mainnet()
        .with_db(db)
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::OSAKA)
        .build_mainnet_with_inspector(inspector)
}

fn transfer_tx(to: Address, value: u64, nonce: u64) -> TxEnv {
    TxEnv {
        caller: SENDER,
        kind: TransactTo::Call(to),
        value: U256::from(value),
        nonce,
        gas_limit: 1_000_000,
        ..Default::default()
    }
}

fn transfer_log(from: Address, to: Address, value: u64) -> Log {
    Log::new_unchecked(
        TRANSFER_LOG_EMITTER,
        vec![TRANSFER_EVENT_TOPIC, from.into_word(), to.into_word()],
        U256::from(value).to_be_bytes::<32>().into(),
    )
}

#[test]
fn collected_logs_preserve_order_and_discard_reverted_frames() {
    for internal_only in [false, true] {
        let mut evm = transfer_evm(
            TransferInspector::new(internal_only).with_log_collection(),
            [
                // LOG0, reverting CALL with value 1, successful CALL with value 2, LOG0.
                (CALLER, hex!("5f5fa05f5f5f5f60017300000000000000000000000000000000000000435af1505f5f5f5f60027300000000000000000000000000000000000000445af1505f5fa000").into(), 0),
                (REVERTING, hex!("5f5fa05f5ffd").into(), 0),
                (RECEIVER, hex!("5f5fa000").into(), 0),
                // A successful child call followed by a parent REVERT.
                (RECIPIENT, hex!("5f5f5f5f60017300000000000000000000000000000000000000445af1505f5ffd").into(), 0),
            ],
        );
        let result = evm.inspect_tx_commit(transfer_tx(CALLER, 5, 0)).unwrap();
        assert!(result.is_success());
        let real_logs = vec![
            Log::new_unchecked(CALLER, vec![], Bytes::new()),
            Log::new_unchecked(RECEIVER, vec![], Bytes::new()),
            Log::new_unchecked(CALLER, vec![], Bytes::new()),
        ];
        assert_eq!(result.logs(), real_logs);
        let mut expected = vec![
            real_logs[0].clone(),
            transfer_log(CALLER, RECEIVER, 2),
            real_logs[1].clone(),
            real_logs[2].clone(),
        ];
        if !internal_only {
            expected.insert(0, transfer_log(SENDER, CALLER, 5));
        }
        assert_eq!(evm.inspector().logs(), expected);
        let transfers = evm.inspector().transfers().to_vec();
        assert_eq!(transfers.len(), if internal_only { 1 } else { 2 });
        assert_eq!(transfers.last().unwrap().to, RECEIVER);

        let result = evm.inspect_tx_commit(transfer_tx(RECIPIENT, 5, 1)).unwrap();
        assert!(!result.is_success());
        assert!(result.logs().is_empty());
        assert!(evm.inspector().logs().is_empty());
        assert_eq!(evm.inspector().transfers(), transfers);

        let result =
            evm.inspect_tx_commit(transfer_tx(Address::with_last_byte(0x48), 1, 2)).unwrap();
        assert!(result.is_success());
        let expected = if internal_only {
            vec![]
        } else {
            vec![transfer_log(SENDER, Address::with_last_byte(0x48), 1)]
        };
        assert_eq!(evm.ctx_inspector().1.take_logs(), expected);
        assert!(evm.inspector().logs().is_empty());
    }
}

#[test]
fn reverted_transfers_are_discarded_in_every_log_mode() {
    for inspector in [
        TransferInspector::new(false),
        TransferInspector::new(false).with_logs(true),
        TransferInspector::new(false).with_log_collection(),
    ] {
        let mut evm = transfer_evm(inspector, [
            // LOG0, reverting CALL with value 1, LOG0.
            (CALLER, hex!("5f5fa05f5f5f5f60017300000000000000000000000000000000000000435af1505f5fa000").into(), 10),
            (REVERTING, hex!("5f5fa05f5ffd").into(), 0),
        ]);
        let result = evm.inspect_tx_commit(transfer_tx(CALLER, 0, 0)).unwrap();
        assert!(result.is_success());
        assert!(evm.inspector().transfers().is_empty());
        assert_eq!(
            result.logs(),
            [
                Log::new_unchecked(CALLER, vec![], Bytes::new()),
                Log::new_unchecked(CALLER, vec![], Bytes::new()),
            ]
        );
    }
}

#[test]
fn callcode_and_delegatecall_do_not_report_transfers() {
    for opcode in [0xf2, 0xf4] {
        let mut code = hex!("5f5f5f5f").to_vec();
        if opcode == 0xf2 {
            code.extend_from_slice(&hex!("6001"));
        }
        code.push(0x73);
        code.extend_from_slice(RECEIVER.as_slice());
        code.extend_from_slice(&[0x5a, opcode, 0x50, 0x00]);
        let mut evm = transfer_evm(
            TransferInspector::new(false).with_log_collection(),
            [(CALLER, code.into(), 0), (RECEIVER, hex!("5f5fa000").into(), 0)],
        );
        let result = evm.inspect_tx_commit(transfer_tx(CALLER, 3, 0)).unwrap();
        assert!(result.is_success());
        assert_eq!(evm.inspector().transfers().len(), 1);
        assert_eq!(
            evm.inspector().logs(),
            [transfer_log(SENDER, CALLER, 3), Log::new_unchecked(CALLER, vec![], Bytes::new()),]
        );
    }
}

#[test]
fn create_logs_follow_success_and_failure_outcomes() {
    let mut evm = transfer_evm(TransferInspector::new(false).with_log_collection(), []);
    let result = evm
        .inspect_tx_commit(TxEnv {
            kind: TransactTo::Create,
            data: hex!("5f5fa05f5ff3").into(),
            ..transfer_tx(RECIPIENT, 1, 0)
        })
        .unwrap();
    assert!(result.is_success());
    assert_eq!(
        evm.inspector().logs(),
        [
            transfer_log(SENDER, SENDER.create(0), 1),
            Log::new_unchecked(SENDER.create(0), vec![], Bytes::new()),
        ]
    );
    assert_eq!(evm.inspector().transfers()[0].kind, TransferKind::Create);
    for (nonce, init_code) in
        [(1, hex!("5f5fa05f5ffd").as_slice()), (2, hex!("5f5fa0fe").as_slice())]
    {
        let result = evm
            .inspect_tx_commit(TxEnv {
                kind: TransactTo::Create,
                data: Bytes::copy_from_slice(init_code),
                ..transfer_tx(RECIPIENT, 1, nonce)
            })
            .unwrap();
        assert!(!result.is_success());
        assert!(result.logs().is_empty());
        assert!(evm.inspector().logs().is_empty());
        assert_eq!(evm.inspector().transfers().len(), 1);
    }
}

#[test]
fn create2_logs_are_ordered_and_reverted_creates_are_discarded() {
    for succeeds in [true, false] {
        let init = if succeeds { hex!("5f5fa05f5ff3") } else { hex!("5f5fa05f5ffd") };
        // Store six bytes of init code at offset 26, then CREATE2 with value 1 and salt 0.
        let mut code = vec![0x65];
        code.extend_from_slice(&init);
        code.extend_from_slice(&hex!("5f525f6006601a6001f55000"));
        let mut evm = transfer_evm(
            TransferInspector::new(false).with_log_collection(),
            [(CALLER, code.into(), 0)],
        );
        let result = evm.inspect_tx_commit(transfer_tx(CALLER, 5, 0)).unwrap();
        assert!(result.is_success());
        let mut expected = vec![transfer_log(SENDER, CALLER, 5)];
        if succeeds {
            let created = CALLER.create2_from_code(B256::ZERO, init);
            expected.push(transfer_log(CALLER, created, 1));
            expected.push(Log::new_unchecked(created, vec![], Bytes::new()));
            assert_eq!(evm.inspector().transfers()[1].kind, TransferKind::Create2);
        }
        assert_eq!(evm.inspector().logs(), expected);
        assert_eq!(evm.inspector().transfers().len(), if succeeds { 2 } else { 1 });
    }
}

#[test]
fn selfdestruct_logs_support_both_modes_and_skip_zero_value() {
    for (inspector, journal_logs) in [
        (TransferInspector::new(false).with_logs(true), true),
        (TransferInspector::new(false).with_log_collection(), false),
        (TransferInspector::new(false).with_log_collection().with_logs(true), true),
        (TransferInspector::new(false).with_logs(true).with_log_collection(), false),
    ] {
        let mut evm = transfer_evm(
            inspector,
            [(CALLER, hex!("730000000000000000000000000000000000000042ff").into(), 10)],
        );
        let result = evm.inspect_tx_commit(transfer_tx(CALLER, 1, 0)).unwrap();
        assert!(result.is_success());
        let expected = [transfer_log(SENDER, CALLER, 1), transfer_log(CALLER, RECIPIENT, 11)];
        if journal_logs {
            assert_eq!(result.logs(), expected);
            assert!(evm.inspector().logs().is_empty());
        } else {
            assert!(result.logs().is_empty());
            assert_eq!(evm.inspector().logs(), expected);
        }
        let result = evm.inspect_tx_commit(transfer_tx(CALLER, 0, 1)).unwrap();
        assert!(result.is_success());
        assert!(result.logs().is_empty());
        assert!(evm.inspector().logs().is_empty());
        assert_eq!(evm.inspector().transfers().len(), 2);
    }
}

#[test]
fn contract_logs_identical_to_synthetic_logs_are_preserved() {
    let expected = transfer_log(SENDER, TRANSFER_LOG_EMITTER, 1);
    // Store the value, push topics in reverse order, then emit LOG3 with 32 bytes of data.
    let mut code = hex!("60015f52").to_vec();
    for topic in expected.topics().iter().rev() {
        code.push(0x7f);
        code.extend_from_slice(topic.as_slice());
    }
    code.extend_from_slice(&hex!("60205fa300"));
    let mut evm = transfer_evm(
        TransferInspector::new(false).with_log_collection(),
        [(TRANSFER_LOG_EMITTER, code.into(), 0)],
    );
    let result = evm.inspect_tx_commit(transfer_tx(TRANSFER_LOG_EMITTER, 1, 0)).unwrap();
    assert!(result.is_success());
    assert_eq!(result.logs(), core::slice::from_ref(&expected));
    assert_eq!(evm.inspector().logs(), [expected.clone(), expected]);
    assert_eq!(evm.inspector().transfers().len(), 1);
}
