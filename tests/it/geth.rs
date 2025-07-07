//! Geth tests
use crate::utils::deploy_contract;
use alloy_primitives::{hex, map::HashMap, Address, Bytes, TxKind};
use alloy_rpc_types_eth::TransactionInfo;
use alloy_rpc_types_trace::geth::{
    mux::MuxConfig, CallConfig, FlatCallConfig, GethDebugBuiltInTracerType, GethDebugTracerConfig,
    GethTrace, PreStateConfig, PreStateFrame,
};
use revm::{
    context::TxEnv,
    context_interface::{ContextTr, TransactTo},
    database::CacheDB,
    database_interface::EmptyDB,
    handler::EvmTr,
    inspector::InspectorEvmTr,
    primitives::hardfork::SpecId,
    Context, InspectEvm, MainBuilder, MainContext,
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
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();
    let code = hex!("608060405234801561001057600080fd5b506103ac806100206000396000f3fe60806040526004361061003f5760003560e01c80630332ed131461014d5780636ae1ad40146101625780638384a00214610177578063de7eb4f31461018c575b60405134815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561009d57600080fd5b505af19250505080156100ae575060015b50306001600160a01b0316630332ed136040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100ea57600080fd5b505af19250505080156100fb575060015b50306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561013757600080fd5b505af115801561014b573d6000803e3d6000fd5b005b34801561015957600080fd5b5061014b6101a1565b34801561016e57600080fd5b5061014b610253565b34801561018357600080fd5b5061014b6102b7565b34801561019857600080fd5b5061014b6102dd565b306001600160a01b031663de7eb4f36040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156101dc57600080fd5b505af11580156101f0573d6000803e3d6000fd5b505060405162461bcd60e51b8152602060048201526024808201527f6e6573746564456d6974576974684661696c75726541667465724e6573746564604482015263115b5a5d60e21b6064820152608401915061024a9050565b60405180910390fd5b6040516000815233906000805160206103578339815191529060200160405180910390a260405162461bcd60e51b81526020600482015260156024820152746e6573746564456d6974576974684661696c75726560581b604482015260640161024a565b6040516000815233906000805160206103578339815191529060200160405180910390a2565b6040516000815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561033c57600080fd5b505af1158015610350573d6000803e3d6000fd5b5050505056fef950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23ea2646970667358221220090a696b9fbd22c7d1cc2a0b6d4a48c32d3ba892480713689a3145b73cfeb02164736f6c63430008130033");
    let deployer = Address::ZERO;
    let addr =
        deploy_contract(&mut evm, code.into(), deployer, SpecId::LONDON).created_address().unwrap();

    let mut insp =
        TracingInspector::new(TracingInspectorConfig::default_geth().set_record_logs(true));

    let mut evm = evm.with_inspector(&mut insp);

    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr),
            data: Bytes::default(), // call fallback
            nonce: 1,
            ..Default::default()
        })
        .unwrap();
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

        function nestedRevert() external {
            this.nestedEmitWithFailure();
        }
    }
    */

    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let code = hex!("608060405234801561001057600080fd5b506103ac806100206000396000f3fe60806040526004361061003f5760003560e01c80630332ed131461014d5780636ae1ad40146101625780638384a00214610177578063de7eb4f31461018c575b60405134815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561009d57600080fd5b505af19250505080156100ae575060015b50306001600160a01b0316630332ed136040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100ea57600080fd5b505af19250505080156100fb575060015b50306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561013757600080fd5b505af115801561014b573d6000803e3d6000fd5b005b34801561015957600080fd5b5061014b6101a1565b34801561016e57600080fd5b5061014b610253565b34801561018357600080fd5b5061014b6102b7565b34801561019857600080fd5b5061014b6102dd565b306001600160a01b031663de7eb4f36040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156101dc57600080fd5b505af11580156101f0573d6000803e3d6000fd5b505060405162461bcd60e51b8152602060048201526024808201527f6e6573746564456d6974576974684661696c75726541667465724e6573746564604482015263115b5a5d60e21b6064820152608401915061024a9050565b60405180910390fd5b6040516000815233906000805160206103578339815191529060200160405180910390a260405162461bcd60e51b81526020600482015260156024820152746e6573746564456d6974576974684661696c75726560581b604482015260640161024a565b6040516000815233906000805160206103578339815191529060200160405180910390a2565b6040516000815233906000805160206103578339815191529060200160405180910390a2306001600160a01b0316638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561033c57600080fd5b505af1158015610350573d6000803e3d6000fd5b5050505056fef950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23ea2646970667358221220090a696b9fbd22c7d1cc2a0b6d4a48c32d3ba892480713689a3145b73cfeb02164736f6c63430008130033");
    let deployer = Address::ZERO;
    let addr =
        deploy_contract(&mut evm, code.into(), deployer, SpecId::LONDON).created_address().unwrap();

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

    let mut evm = evm.with_inspector(&mut insp);

    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr),
            data: Bytes::default(), // call fallback
            nonce: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(res.result.is_success());

    let (ctx, inspector) = evm.ctx_inspector();
    let frame =
        inspector.try_into_mux_frame(&res, ctx.db_ref(), TransactionInfo::default()).unwrap();

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
    let insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    let context = Context::mainnet()
        .with_db(CacheDB::new(EmptyDB::default()))
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::LONDON);

    assert_eq!(insp.traces().nodes().first().unwrap().trace.gas_limit, 0);

    let mut evm = context.build_mainnet_with_inspector(insp);
    let tx = TxEnv::builder()
        .caller(Address::ZERO)
        .gas_limit(1000000)
        .gas_price(Default::default())
        .kind(TxKind::Call(Address::ZERO))
        .build_fill();
    // first run inspector
    let res = evm.inspect_tx(tx.clone()).unwrap();
    assert!(res.result.is_success());
    assert_eq!(
        evm.inspector()
            .clone()
            .with_transaction_gas_limit(evm.ctx().tx().gas_limit)
            .traces()
            .nodes()
            .first()
            .unwrap()
            .trace
            .gas_limit,
        1000000
    );

    // reset the inspector
    evm.inspector().fuse();
    assert_eq!(evm.inspector().traces().nodes().first().unwrap().trace.gas_limit, 0);

    // second run inspector after reset
    let res = evm.inspect_tx(tx).unwrap();
    assert!(res.result.is_success());
    let gas_limit = evm.ctx().tx().gas_limit;
    assert_eq!(
        evm.into_inspector()
            .with_transaction_gas_limit(gas_limit)
            .traces()
            .nodes()
            .first()
            .unwrap()
            .trace
            .gas_limit,
        1000000
    );
}

#[test]
fn test_geth_calltracer_top_call_reverting() {
    /*
    Test that verifies the behavior of only_top_call with a reverting transaction.
    Uses the LogTracing contract which has functions that make nested calls and revert.
    */
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    // Use the LogTracing contract from test_geth_calltracer_logs
    let code = hex!("0x608060405234801561001057600080fd5b50610694806100206000396000f3fe60806040526004361061004e5760003560e01c80630332ed13146101af5780636ae1ad40146101c65780638384a002146101dd578063c8cc8494146101f4578063de7eb4f31461020b5761004f565b5b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e3460405161009591906104d4565b60405180910390a23073ffffffffffffffffffffffffffffffffffffffff16636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100e557600080fd5b505af19250505080156100f6575060015b503073ffffffffffffffffffffffffffffffffffffffff16630332ed136040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561013f57600080fd5b505af1925050508015610150575060015b503073ffffffffffffffffffffffffffffffffffffffff16638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561019957600080fd5b505af11580156101ad573d6000803e3d6000fd5b005b3480156101bb57600080fd5b506101c4610222565b005b3480156101d257600080fd5b506101db6102c5565b005b3480156101e957600080fd5b506101f2610357565b005b34801561020057600080fd5b506102096103a8565b005b34801561021757600080fd5b5061022061040a565b005b3073ffffffffffffffffffffffffffffffffffffffff1663de7eb4f36040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561026a57600080fd5b505af115801561027e573d6000803e3d6000fd5b5050505060006102c3576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016102ba90610572565b60405180910390fd5b565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e600060405161030c91906105d7565b60405180910390a26000610355576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161034c9061063e565b60405180910390fd5b565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e600060405161039e91906105d7565b60405180910390a2565b3073ffffffffffffffffffffffffffffffffffffffff16636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156103f057600080fd5b505af1158015610404573d6000803e3d6000fd5b50505050565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e600060405161045191906105d7565b60405180910390a23073ffffffffffffffffffffffffffffffffffffffff16638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156104a157600080fd5b505af11580156104b5573d6000803e3d6000fd5b50505050565b6000819050919050565b6104ce816104bb565b82525050565b60006020820190506104e960008301846104c5565b92915050565b600082825260208201905092915050565b7f6e6573746564456d6974576974684661696c75726541667465724e657374656460008201527f456d697400000000000000000000000000000000000000000000000000000000602082015250565b600061055c6024836104ef565b915061056782610500565b604082019050919050565b6000602082019050818103600083015261058b8161054f565b9050919050565b6000819050919050565b6000819050919050565b60006105c16105bc6105b784610592565b61059c565b6104bb565b9050919050565b6105d1816105a6565b82525050565b60006020820190506105ec60008301846105c8565b92915050565b7f6e6573746564456d6974576974684661696c7572650000000000000000000000600082015250565b60006106286015836104ef565b9150610633826105f2565b602082019050919050565b600060208201905081810360008301526106578161061b565b905091905056fea264697066735822122071d074b2a07496c0c9168e0a9fa623892624715f6fb50195649cbea96e486eed64736f6c634300080d0033");
    let deployer = Address::ZERO;
    let addr =
        deploy_contract(&mut evm, code.into(), deployer, SpecId::LONDON).created_address().unwrap();

    // Test with only_top_call = true on a transaction that has nested calls and reverts
    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());
    let mut evm = evm.with_inspector(&mut insp);

    // Call nestedEmitWithFailureAfterNestedEmit which has nested calls before reverting
    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr),
            data: hex!("0332ed13").into(), // nestedEmitWithFailureAfterNestedEmit() selector
            nonce: 1,
            ..Default::default()
        })
        .unwrap();

    assert!(!res.result.is_success());

    // Get call traces with only_top_call = true
    let call_config_top = CallConfig { only_top_call: Some(true), with_log: Some(false) };
    let call_frame_top = insp
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_call_traces(call_config_top, res.result.gas_used());

    // With only_top_call = true, we should not see any subcalls in the trace
    assert_eq!(call_frame_top.calls.len(), 0, "Should have no subcalls when only_top_call is true");
    assert!(call_frame_top.error.is_some(), "Top call should have an error");

    // Now test with only_top_call = false to verify we see the nested structure
    let mut insp2 = TracingInspector::new(TracingInspectorConfig::default_geth());
    let mut evm2 = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let addr2 = deploy_contract(&mut evm2, code.into(), deployer, SpecId::LONDON)
        .created_address()
        .unwrap();
    let mut evm2 = evm2.with_inspector(&mut insp2);

    let res2 = evm2
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr2),
            data: hex!("0332ed13").into(), // nestedEmitWithFailureAfterNestedEmit() selector
            nonce: 1,
            ..Default::default()
        })
        .unwrap();

    assert!(!res2.result.is_success());

    // Get call traces with only_top_call = false (default)
    let call_config_all = CallConfig { only_top_call: Some(false), with_log: Some(false) };
    let call_frame_all = insp2
        .with_transaction_gas_used(res2.result.gas_used())
        .geth_builder()
        .geth_call_traces(call_config_all, res2.result.gas_used());

    // nestedEmitWithFailureAfterNestedEmit calls doubleNestedEmitWithSuccess which calls
    // nestedEmitWithSuccess So we should see nested calls when only_top_call = false
    assert_eq!(
        call_frame_all.calls.len(),
        1,
        "Should have one subcall for doubleNestedEmitWithSuccess"
    );
    assert!(call_frame_all.error.is_some(), "Top call should have an error");
    assert!(call_frame_all.calls[0].error.is_none(), "doubleNestedEmitWithSuccess should succeed");
    assert_eq!(
        call_frame_all.calls[0].calls.len(),
        1,
        "doubleNestedEmitWithSuccess should call nestedEmitWithSuccess"
    );
}

#[test]
fn test_geth_calltracer_nested_revert() {
    /*
    Test that verifies the behavior of only_top_call with the nestedRevert function.
    This function calls nestedEmitWithFailure which emits a log and then reverts.
    */
    let mut evm = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    // Use the LogTracing contract with nestedRevert function
    let code = hex!("608060405234801561001057600080fd5b50610694806100206000396000f3fe60806040526004361061004e5760003560e01c80630332ed13146101af5780636ae1ad40146101c65780638384a002146101dd578063c8cc8494146101f4578063de7eb4f31461020b5761004f565b5b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e3460405161009591906104d4565b60405180910390a23073ffffffffffffffffffffffffffffffffffffffff16636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156100e557600080fd5b505af19250505080156100f6575060015b503073ffffffffffffffffffffffffffffffffffffffff16630332ed136040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561013f57600080fd5b505af1925050508015610150575060015b503073ffffffffffffffffffffffffffffffffffffffff16638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561019957600080fd5b505af11580156101ad573d6000803e3d6000fd5b005b3480156101bb57600080fd5b506101c4610222565b005b3480156101d257600080fd5b506101db6102c5565b005b3480156101e957600080fd5b506101f2610357565b005b34801561020057600080fd5b506102096103a8565b005b34801561021757600080fd5b5061022061040a565b005b3073ffffffffffffffffffffffffffffffffffffffff1663de7eb4f36040518163ffffffff1660e01b8152600401600060405180830381600087803b15801561026a57600080fd5b505af115801561027e573d6000803e3d6000fd5b5050505060006102c3576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016102ba90610572565b60405180910390fd5b565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e600060405161030c91906105d7565b60405180910390a26000610355576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161034c9061063e565b60405180910390fd5b565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e600060405161039e91906105d7565b60405180910390a2565b3073ffffffffffffffffffffffffffffffffffffffff16636ae1ad406040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156103f057600080fd5b505af1158015610404573d6000803e3d6000fd5b50505050565b3373ffffffffffffffffffffffffffffffffffffffff167ff950957d2407bed19dc99b718b46b4ce6090c05589006dfb86fd22c34865b23e600060405161045191906105d7565b60405180910390a23073ffffffffffffffffffffffffffffffffffffffff16638384a0026040518163ffffffff1660e01b8152600401600060405180830381600087803b1580156104a157600080fd5b505af11580156104b5573d6000803e3d6000fd5b50505050565b6000819050919050565b6104ce816104bb565b82525050565b60006020820190506104e960008301846104c5565b92915050565b600082825260208201905092915050565b7f6e6573746564456d6974576974684661696c75726541667465724e657374656460008201527f456d697400000000000000000000000000000000000000000000000000000000602082015250565b600061055c6024836104ef565b915061056782610500565b604082019050919050565b6000602082019050818103600083015261058b8161054f565b9050919050565b6000819050919050565b6000819050919050565b60006105c16105bc6105b784610592565b61059c565b6104bb565b9050919050565b6105d1816105a6565b82525050565b60006020820190506105ec60008301846105c8565b92915050565b7f6e6573746564456d6974576974684661696c7572650000000000000000000000600082015250565b60006106286015836104ef565b9150610633826105f2565b602082019050919050565b600060208201905081810360008301526106578161061b565b905091905056fea264697066735822122071d074b2a07496c0c9168e0a9fa623892624715f6fb50195649cbea96e486eed64736f6c634300080d0033");
    let deployer = Address::ZERO;
    let addr =
        deploy_contract(&mut evm, code.into(), deployer, SpecId::LONDON).created_address().unwrap();

    // Test with only_top_call = true
    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());
    let mut evm = evm.with_inspector(&mut insp);

    // Call nestedRevert which calls nestedEmitWithFailure
    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr),
            data: hex!("c8cc8494").into(), // nestedRevert() selector
            nonce: 1,
            ..Default::default()
        })
        .unwrap();

    assert!(!res.result.is_success());

    // Get call traces with only_top_call = true
    let call_config_top = CallConfig { only_top_call: Some(true), with_log: Some(false) };
    let call_frame_top = insp
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_call_traces(call_config_top, res.result.gas_used());

    // With only_top_call = true, we should not see the subcall to nestedEmitWithFailure
    assert_eq!(call_frame_top.calls.len(), 0, "Should have no subcalls when only_top_call is true");
    assert!(call_frame_top.error.is_some(), "Top call should have an error");

    // Now test with only_top_call = false
    let mut insp2 = TracingInspector::new(TracingInspectorConfig::default_geth());
    let mut evm2 = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let addr2 = deploy_contract(&mut evm2, code.into(), deployer, SpecId::LONDON)
        .created_address()
        .unwrap();
    let mut evm2 = evm2.with_inspector(&mut insp2);

    let res2 = evm2
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr2),
            data: hex!("c8cc8494").into(), // nestedRevert() selector
            nonce: 1,
            ..Default::default()
        })
        .unwrap();

    assert!(!res2.result.is_success());

    // Get call traces with only_top_call = false
    let call_config_all = CallConfig { only_top_call: Some(false), with_log: Some(false) };
    let call_frame_all = insp2
        .with_transaction_gas_used(res2.result.gas_used())
        .geth_builder()
        .geth_call_traces(call_config_all, res2.result.gas_used());

    // nestedRevert calls nestedEmitWithFailure, so we should see one subcall
    assert_eq!(call_frame_all.calls.len(), 1, "Should have one subcall to nestedEmitWithFailure");
    assert!(call_frame_all.error.is_some(), "Top call should have an error");
    assert!(
        call_frame_all.calls[0].error.is_some(),
        "nestedEmitWithFailure should also have an error"
    );

    // Test revert with topcall
    let mut insp3 =
        TracingInspector::new(TracingInspectorConfig::default_geth().set_record_logs(true));
    let mut evm3 = Context::mainnet().with_db(CacheDB::new(EmptyDB::default())).build_mainnet();

    let addr3 = deploy_contract(&mut evm3, code.into(), deployer, SpecId::LONDON)
        .created_address()
        .unwrap();
    let mut evm3 = evm3.with_inspector(&mut insp3);

    let res3 = evm3
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Call(addr3),
            data: hex!("c8cc8494").into(), // nestedRevert() selector
            nonce: 1,
            ..Default::default()
        })
        .unwrap();

    assert!(!res3.result.is_success());

    // Get call traces with logs enabled and only_top_call = false
    let call_config_logs = CallConfig { only_top_call: Some(true), with_log: Some(true) };
    let top_call = insp3
        .with_transaction_gas_used(res3.result.gas_used())
        .geth_builder()
        .geth_call_traces(call_config_logs, res3.result.gas_used());

    // nestedEmitWithFailure emits a log before reverting, but since it reverts, the log should not
    // be included
    assert!(top_call.calls.is_empty(), "Should have no subcalls");
    assert!(top_call.logs.is_empty(), "Top call logs should be empty because it reverted");
    assert!(top_call.error.is_some(), "Top call should have an error");
    assert!(top_call.revert_reason.is_some(), "Top call should have a revert reason");
}
