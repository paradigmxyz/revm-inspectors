//! Appearance inspector tests

use alloy_primitives::{address, bytes, hex, Address, Log};
use revm::{
    bytecode::Bytecode, context::TxEnv, context_interface::TransactTo, database::CacheDB,
    database_interface::EmptyDB, state::AccountInfo, Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::appearance::{
    AddressAppearance, AppearanceInspector, AppearanceLocation, BlockField,
};

#[test]
fn test_appearance_inspector_records_transaction_addresses() {
    let caller = address!("1111111111111111111111111111111111111111");
    let contract = address!("2222222222222222222222222222222222222222");
    let calldata_address = address!("3333333333333333333333333333333333333333");
    let output_address = address!("4444444444444444444444444444444444444444");

    let code = return_address_bytecode(output_address);
    let context =
        Context::mainnet().with_db(CacheDB::<EmptyDB>::default()).modify_db_chained(|db| {
            db.insert_account_info(
                contract,
                AccountInfo { code: Some(Bytecode::new_raw(code)), ..Default::default() },
            );
        });

    let mut inspector = AppearanceInspector::new();
    inspector.set_transaction_index(5);

    let mut evm = context.build_mainnet().with_inspector(&mut inspector);
    let res = evm
        .inspect_tx(TxEnv {
            caller,
            gas_limit: 100000,
            kind: TransactTo::Call(contract),
            data: calldata_with_address(calldata_address),
            nonce: 0,
            ..Default::default()
        })
        .unwrap();
    assert!(res.result.is_success(), "{res:#?}");

    let tx_location = AppearanceLocation::Transaction(5);
    for address in [caller, contract, calldata_address, output_address] {
        assert!(inspector.addresses().contains(&address));
        assert!(inspector.appearances().contains(&AddressAppearance::new(address, tx_location)));
    }
}

#[test]
fn test_appearance_inspector_records_logs_and_block_fields() {
    let emitter = address!("5555555555555555555555555555555555555555");
    let topic_address = address!("6666666666666666666666666666666666666666");
    let data_address = address!("7777777777777777777777777777777777777777");
    let miner = address!("8888888888888888888888888888888888888888");
    let data = address_word(data_address);

    let log = Log::new_unchecked(
        emitter,
        vec![address_word(topic_address)],
        data.as_slice().to_vec().into(),
    );

    let mut inspector = AppearanceInspector::new();
    inspector.set_transaction_index(1);
    inspector.record_log(&log);
    inspector.record_block_field(miner, BlockField::Miner);

    let tx_location = AppearanceLocation::Transaction(1);
    for address in [emitter, topic_address, data_address] {
        assert!(inspector.addresses().contains(&address));
        assert!(inspector.appearances().contains(&AddressAppearance::new(address, tx_location)));
    }

    assert!(inspector.addresses().contains(&miner));
    assert!(inspector.appearances().contains(&AddressAppearance::new(
        miner,
        AppearanceLocation::BlockField(BlockField::Miner),
    )));
}

#[test]
fn test_appearance_inspector_skips_context_precompiles() {
    let caller = address!("9999999999999999999999999999999999999999");
    let precompile = Address::with_last_byte(1);

    let context = Context::mainnet().with_db(CacheDB::<EmptyDB>::default());

    let mut inspector = AppearanceInspector::new();
    inspector.set_transaction_index(0);

    let mut evm = context.build_mainnet().with_inspector(&mut inspector);
    let res = evm
        .inspect_tx(TxEnv {
            caller,
            gas_limit: 100000,
            kind: TransactTo::Call(precompile),
            nonce: 0,
            ..Default::default()
        })
        .unwrap();
    assert!(res.result.is_success(), "{res:#?}");

    assert!(inspector.addresses().contains(&caller));
    assert!(!inspector.addresses().contains(&precompile));
    assert!(!inspector
        .appearances()
        .contains(&AddressAppearance::new(precompile, AppearanceLocation::Transaction(0),)));
}

fn address_word(address: Address) -> alloy_primitives::B256 {
    let mut word = [0; 32];
    word[12..].copy_from_slice(address.as_slice());
    word.into()
}

fn calldata_with_address(address: Address) -> alloy_primitives::Bytes {
    let mut input = bytes!("deadbeef").to_vec();
    input.extend_from_slice(address_word(address).as_slice());
    input.into()
}

fn return_address_bytecode(address: Address) -> alloy_primitives::Bytes {
    let mut code = hex!("73").to_vec();
    code.extend_from_slice(address.as_slice());
    code.extend_from_slice(&hex!("5f5260205ff3"));
    code.into()
}
