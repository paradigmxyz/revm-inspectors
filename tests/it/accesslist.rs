//! Accesslist tests

use alloy_primitives::{address, hex};
use revm::{
    bytecode::Bytecode, context::TxEnv, context_interface::TransactTo, database::CacheDB,
    database_interface::EmptyDB, handler::EvmTr, state::AccountInfo, Context, InspectEvm,
    MainBuilder, MainContext,
};
use revm_inspectors::access_list::AccessListInspector;

#[test]
fn test_access_list_precompile() {
    /*
    contract Storage {
       function recoverSignature() public view returns (address) {
            address r = ecrecover(bytes32(0), 0, 0, 0);
        }
    }
    */

    let code = hex!("608060405234801561000f575f80fd5b5060043610610029575f3560e01c8063a53997051461002d575b5f80fd5b61003561004b565b60405161004291906100e0565b60405180910390f35b5f8060015f801b5f805f6040515f8152602001604052604051610071949392919061019a565b6020604051602081039080840390855afa158015610091573d5f803e3d5ffd5b5050506020604051035190505090565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6100ca826100a1565b9050919050565b6100da816100c0565b82525050565b5f6020820190506100f35f8301846100d1565b92915050565b5f819050919050565b61010b816100f9565b82525050565b5f819050919050565b5f60ff82169050919050565b5f819050919050565b5f61014961014461013f84610111565b610126565b61011a565b9050919050565b6101598161012f565b82525050565b5f815f1b9050919050565b5f61018461017f61017a84610111565b61015f565b6100f9565b9050919050565b6101948161016a565b82525050565b5f6080820190506101ad5f830187610102565b6101ba6020830186610150565b6101c7604083018561018b565b6101d4606083018461018b565b9594505050505056fea26469706673582212208a19ad28dde042d3a2dd7ca8800d08fed7eb780b9778cd88f1e5ab44407532de64736f6c634300081a0033");

    let account = address!("341348115259a8bf69f1f50101c227fced83bac6");
    let caller = address!("341348115259a8bf69f1f50101c227fced83bac5");

    let context =
        Context::mainnet().with_db(CacheDB::<EmptyDB>::default()).modify_db_chained(|db| {
            db.insert_account_info(
                account,
                AccountInfo { code: Some(Bytecode::new_raw(code.into())), ..Default::default() },
            );
        });

    let mut evm = context.build_mainnet();

    evm.ctx().modify_tx(|tx| {
        *tx = TxEnv {
            caller,
            gas_limit: 1000000,
            kind: TransactTo::Call(account),
            data: hex!("a5399705").into(),
            nonce: 0,
            ..Default::default()
        }
    });
    let mut accesslist = AccessListInspector::default();
    let mut evm = evm.with_inspector(&mut accesslist);
    let res = evm.inspect_replay().unwrap();
    assert!(res.result.is_success(), "{res:#?}");

    let erecover = address!("0x0000000000000000000000000000000000000001");
    assert!(accesslist.excluded().contains(&erecover));
    assert!(accesslist.into_access_list().is_empty());
}
