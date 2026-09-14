//! Otterscan internal operations tests.

use alloy_primitives::{hex, Address, Bytes, TxKind, U256};
use alloy_rpc_types_trace::otterscan::{InternalOperation, OperationType};
use revm::{
    context::TxEnv,
    context_interface::result::ExecutionResult,
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId,
    state::{AccountInfo, Bytecode},
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::otterscan::InternalOperationsInspector;

const CONTRACT: Address = Address::repeat_byte(0x11);

fn execute(code: Bytes, spec: SpecId, kind: TxKind) -> (ExecutionResult, Vec<InternalOperation>) {
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        Address::ZERO,
        AccountInfo { balance: U256::from(1_000_000), ..Default::default() },
    );
    db.insert_account_info(
        CONTRACT,
        AccountInfo {
            balance: U256::from(100),
            code: Some(Bytecode::new_legacy(code.clone())),
            ..Default::default()
        },
    );
    let mut inspector = InternalOperationsInspector::default();
    let result = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = spec)
        .with_db(db)
        .build_mainnet_with_inspector(&mut inspector)
        .inspect_tx(TxEnv {
            kind,
            data: if kind.is_create() { code } else { Bytes::new() },
            value: U256::from(7),
            gas_limit: 1_000_000,
            ..Default::default()
        })
        .unwrap();
    (result.result, inspector.into_operations())
}

#[test]
fn excludes_top_level_calls_and_creations() {
    for kind in [TxKind::Call(CONTRACT), TxKind::Create] {
        let (result, operations) = execute(hex!("00").into(), SpecId::CANCUN, kind);
        assert!(result.is_success());
        assert!(operations.is_empty());
    }
}

#[test]
fn includes_zero_value_creates() {
    // CREATE and CREATE2 with empty initcode and zero endowment.
    let (result, operations) = execute(
        hex!("600060006000f0506000600060006000f55000").into(),
        SpecId::CANCUN,
        TxKind::Call(CONTRACT),
    );
    assert!(result.is_success());
    assert_eq!(
        operations,
        [
            InternalOperation {
                from: CONTRACT,
                to: CONTRACT.create(0),
                value: U256::ZERO,
                r#type: OperationType::OpCreate,
            },
            InternalOperation {
                from: CONTRACT,
                to: CONTRACT.create2_from_code([0; 32], []),
                value: U256::ZERO,
                r#type: OperationType::OpCreate2,
            },
        ]
    );
}

#[test]
fn excludes_calls_without_transfers() {
    // Zero-value CALL, STATICCALL, DELEGATECALL, and value-bearing CALLCODE to 0x22.
    let (result, operations) = execute(
        hex!("60006000600060006000602261fffff1506000600060006000602261fffffa506000600060006000602261fffff45060006000600060006001602261fffff25000").into(),
        SpecId::CANCUN,
        TxKind::Call(CONTRACT),
    );
    assert!(result.is_success());
    assert!(operations.is_empty());
}

#[test]
fn retains_reverted_transfers() {
    // Transfer 1 wei to 0x22, then revert the enclosing transaction.
    let (result, operations) = execute(
        hex!("60006000600060006001602261fffff15060006000fd").into(),
        SpecId::CANCUN,
        TxKind::Call(CONTRACT),
    );
    assert!(!result.is_success());
    assert_eq!(
        operations,
        [InternalOperation {
            from: CONTRACT,
            to: Address::with_last_byte(0x22),
            value: U256::from(1),
            r#type: OperationType::OpTransfer,
        }]
    );
}

#[test]
fn retains_failed_creations() {
    // CREATE with initcode that immediately executes INVALID.
    let (result, operations) = execute(
        hex!("60fe600053600160006000f05000").into(),
        SpecId::CANCUN,
        TxKind::Call(CONTRACT),
    );
    assert!(result.is_success());
    assert_eq!(
        operations,
        [InternalOperation {
            from: CONTRACT,
            to: CONTRACT.create(0),
            value: U256::ZERO,
            r#type: OperationType::OpCreate,
        }]
    );
}

#[test]
fn records_selfdestruct_before_and_after_cancun() {
    for spec in [SpecId::SHANGHAI, SpecId::CANCUN] {
        let (result, operations) = execute(hex!("6022ff").into(), spec, TxKind::Call(CONTRACT));
        assert!(result.is_success());
        assert_eq!(
            operations,
            [InternalOperation {
                from: CONTRACT,
                to: Address::with_last_byte(0x22),
                value: U256::from(107),
                r#type: OperationType::OpSelfDestruct,
            }]
        );
    }
}

#[test]
fn preserves_execution_order() {
    // Create a contract whose initcode self-destructs, transfer 1 wei, then self-destruct.
    let (result, operations) = execute(
        hex!("626022ff6000526003601d6000f05060006000600060006001603361fffff1506044ff").into(),
        SpecId::CANCUN,
        TxKind::Call(CONTRACT),
    );
    assert!(result.is_success());
    assert_eq!(
        operations,
        [
            InternalOperation {
                from: CONTRACT,
                to: CONTRACT.create(0),
                value: U256::ZERO,
                r#type: OperationType::OpCreate,
            },
            InternalOperation {
                from: CONTRACT.create(0),
                to: Address::with_last_byte(0x22),
                value: U256::ZERO,
                r#type: OperationType::OpSelfDestruct,
            },
            InternalOperation {
                from: CONTRACT,
                to: Address::with_last_byte(0x33),
                value: U256::from(1),
                r#type: OperationType::OpTransfer,
            },
            InternalOperation {
                from: CONTRACT,
                to: Address::with_last_byte(0x44),
                value: U256::from(106),
                r#type: OperationType::OpSelfDestruct,
            },
        ]
    );
}
