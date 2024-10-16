//! Geth Js tracer tests

use crate::utils::{deploy_contract, inspect};
use alloy_primitives::{hex, Address};
use revm::primitives::{SpecId, TransactTo, TxEnv};
use revm_inspectors::tracing::js::JsInspector;

#[test]
fn test_geth_jstracer_revert() {
    /*
    pragma solidity ^0.8.13;

    contract Foo {
        event Log(address indexed addr, uint256 value);

        function foo() external {
            emit Log(msg.sender, 0);
        }

        function bar() external {
            emit Log(msg.sender, 0);
            require(false, "barbarbar");
        }
    }
    */

    let code = hex!("608060405261023e806100115f395ff3fe608060405234801561000f575f80fd5b5060043610610034575f3560e01c8063c298557814610038578063febb0f7e14610042575b5f80fd5b61004061004c565b005b61004a61009c565b005b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e5f6040516100929190610177565b60405180910390a2565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e5f6040516100e29190610177565b60405180910390a25f61012a576040517f08c379a0000000000000000000000000000000000000000000000000000000008152600401610121906101ea565b60405180910390fd5b565b5f819050919050565b5f819050919050565b5f819050919050565b5f61016161015c6101578461012c565b61013e565b610135565b9050919050565b61017181610147565b82525050565b5f60208201905061018a5f830184610168565b92915050565b5f82825260208201905092915050565b7f62617262617262617200000000000000000000000000000000000000000000005f82015250565b5f6101d4600983610190565b91506101df826101a0565b602082019050919050565b5f6020820190508181035f830152610201816101c8565b905091905056fea2646970667358221220e058dc2c4bd629d62405850cc8e08e6bfad0eea187260784445dfe8f3ee0bea564736f6c634300081a0033");
    let deployer = Address::ZERO;

    let (addr, mut evm) = deploy_contract(code.into(), deployer, SpecId::CANCUN);

    let code = r#"
{
    fault: function() {},
    result: function(ctx) { return { error: !!ctx.error }; },
}"#;

    // test with normal operation
    let mut env = evm.env_with_tx(TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        transact_to: TransactTo::Call(addr),
        data: hex!("c2985578").into(), // call foo
        ..Default::default()
    });

    let mut insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
    let (res, _) = inspect(&mut evm.db, env.clone(), &mut insp).unwrap();
    assert!(res.result.is_success());

    let result = insp.json_result(res, &env, &evm.db).unwrap();

    // sucessful operation
    assert!(!result["error"].as_bool().unwrap());

    // test with reverted operation
    env.tx = TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        transact_to: TransactTo::Call(addr),
        data: hex!("febb0f7e").into(), // call bar
        ..Default::default()
    };
    let mut insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
    let (res, _) = inspect(&mut evm.db, env.clone(), &mut insp).unwrap();
    assert!(!res.result.is_success());

    let result = insp.json_result(res, &env, &evm.db).unwrap();

    // reverted operation
    assert!(result["error"].as_bool().unwrap());
}
