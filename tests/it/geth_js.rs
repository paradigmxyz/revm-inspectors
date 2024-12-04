//! Geth Js tracer tests

use crate::utils::{deploy_contract, inspect, TestEvm};
use alloy_primitives::{address, hex, Address};
use revm::primitives::{SpecId, TransactTo, TxEnv};
use revm_inspectors::tracing::js::JsInspector;
use serde_json::json;

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

// Fix issue https://github.com/paradigmxyz/reth/issues/13089
#[test]
fn test_geth_jstracer_proxy_contract() {
    /*

    contract Token {
        event Transfer(address indexed from, address indexed to, uint256 value);

        function transfer(address to, uint256 amount) public payable {
            emit Transfer(msg.sender, to, amount);
        }
    }

    contract Proxy {
        function transfer(address _contract, address _to, uint256 _amount) external payable {
            (bool success, bytes memory data) =
                _contract.delegatecall(abi.encodeWithSignature("transfer(address,uint256)", _to, _amount));
            require(success, "failed to delegatecall");
        }
    }
    */

    let token_code = hex!("6080604052348015600e575f80fd5b5060dd80601a5f395ff3fe608060405260043610601b575f3560e01c8063a9059cbb14601f575b5f80fd5b602e602a3660046074565b6030565b005b6040518181526001600160a01b0383169033907fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef9060200160405180910390a35050565b5f80604083850312156084575f80fd5b82356001600160a01b03811681146099575f80fd5b94602093909301359350505056fea2646970667358221220d81408f997c5f148e7d6afc66ccc7cda17a38396925363f11993fa885b70729b64736f6c63430008190033");

    let proxy_code = hex!("6080604052348015600e575f80fd5b506101998061001c5f395ff3fe60806040526004361061001d575f3560e01c80631a69523014610021575b5f80fd5b61003461002f366004610120565b610036565b005b6040516104006024820152606560448201525f9081906001600160a01b0384169060640160408051601f198184030181529181526020820180516001600160e01b031663a9059cbb60e01b1790525161008f919061014d565b5f60405180830381855af49150503d805f81146100c7576040519150601f19603f3d011682016040523d82523d5f602084013e6100cc565b606091505b50915091508161011b5760405162461bcd60e51b815260206004820152601660248201527519985a5b1959081d1bc819195b1959d85d1958d85b1b60521b604482015260640160405180910390fd5b505050565b5f60208284031215610130575f80fd5b81356001600160a01b0381168114610146575f80fd5b9392505050565b5f82518060208501845e5f92019182525091905056fea2646970667358221220d7855999519e998c7bcef0432918ca2f5b00228a4058ba259260e327013226f764736f6c63430008190033");

    let deployer = address!("f077b491b355e64048ce21e3a6fc4751eeea77fa");

    let mut evm = TestEvm::new();

    // Deploy Implementation(Token) contract
    let token_addr = evm.simple_deploy(token_code.into());

    // Deploy Proxy contract
    let proxy_addr = evm.simple_deploy(proxy_code.into());

    // Set input data for ProxyFactory.transfer(address)
    let mut input_data = hex!("1a695230").to_vec(); // keccak256("transfer(address)")[:4]
    input_data.extend_from_slice(&[0u8; 12]); // Pad with zeros
    input_data.extend_from_slice(token_addr.as_slice());
    println!("token {:?} proxy: {:?}", token_addr, proxy_addr);

    let code = r#"
{
    data: [],
    fault: function(log) {},
    step: function(log) {
        if (log.op.toString().match(/LOG/)) {
            const topic1 = log.stack.peek(2).toString(16);
            const caller = toHex(log.contract.getCaller());
            const token = toHex(log.contract.getAddress());
            if (topic1 === "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef") {
                this.data.push({ event: "Transfer", token, caller })
            }
        }
    },
    result: function() { return this.data; }
}"#;
    let mut insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
    let env = evm.env_with_tx(TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        transact_to: TransactTo::Call(proxy_addr),
        data: input_data.into(),
        ..Default::default()
    });

    let (res, _) = inspect(&mut evm.db, env.clone(), &mut insp).unwrap();
    assert!(res.result.is_success());

    let result = insp.json_result(res, &env, &evm.db).unwrap();
    assert_eq!(result, json!([{"event": "Transfer", "token": proxy_addr, "caller": deployer}]));
}
