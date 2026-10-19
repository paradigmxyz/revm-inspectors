//! Call tracer regressions for halts before revm initializes the root frame.

use super::*;
use alloy_rpc_types_trace::geth::{CallFrame, CallLogFrame};
use revm::context_interface::result::{ExecutionResult, HaltReason};

#[test]
fn runtime_oog_call_frames_and_reuse() {
    let caller = address!("1000000000000000000000000000000000000001");
    let recipient = address!("1000000000000000000000000000000000000002");
    for mux in [false, true] {
        for only_top_call in [false, true] {
            let config = serde_json::json!({ "onlyTopCall": only_top_call, "withLog": true });
            let options = if mux {
                serde_json::json!({"tracer": "muxTracer", "tracerConfig": {"callTracer": config}})
            } else {
                serde_json::json!({"tracer": "callTracer", "tracerConfig": config})
            };
            let inspector = DebugInspector::new(serde_json::from_value(options).unwrap()).unwrap();
            let context = Context::mainnet()
                .with_db(CacheDB::<EmptyDB>::default())
                .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::AMSTERDAM))
                .modify_db_chained(|db| {
                    db.insert_account_info(
                        caller,
                        AccountInfo::from_balance(U256::from(1_000_000)),
                    );
                });
            let mut evm = context.build_mainnet().with_inspector(inspector);

            // Alternate recorded and missing roots to catch stale state across fuse(). The
            // successful empty-code call also uses its full gas limit without executing opcodes.
            for (gas_limit, input, kind, halted) in [
                (200_000, Bytes::new(), TxKind::Call(recipient), true),
                (204_600, Bytes::new(), TxKind::Call(recipient), false),
                (200_000, hex!("01020304").into(), TxKind::Call(recipient), true),
                (200_000, hex!("00").into(), TxKind::Create, true),
                (204_600, Bytes::new(), TxKind::Call(recipient), false),
            ] {
                let tx = TxEnv {
                    caller,
                    kind,
                    value: U256::ONE,
                    data: input.clone(),
                    gas_limit,
                    ..Default::default()
                };
                let res = evm.inspect_tx(tx.clone()).unwrap();
                assert_eq!(res.result.is_halt(), halted);
                assert_eq!(res.result.tx_gas_used(), gas_limit);
                let (ctx, inspector) = evm.ctx_inspector();
                let block = ctx.block().clone();
                let trace = inspector.get_result(None, &tx, &block, &res, ctx.db_mut()).unwrap();
                // Result construction is idempotent.
                let again = inspector.get_result(None, &tx, &block, &res, ctx.db_mut()).unwrap();
                assert_eq!(
                    serde_json::to_value(&trace).unwrap(),
                    serde_json::to_value(again).unwrap()
                );
                let json = serde_json::to_value(trace).unwrap();
                let frame = if mux { json["callTracer"].clone() } else { json };
                let expected = CallFrame {
                    from: caller,
                    to: (!kind.is_create()).then_some(recipient),
                    typ: if kind.is_create() { "CREATE" } else { "CALL" }.into(),
                    value: Some(U256::ONE),
                    input,
                    gas: U256::from(gas_limit),
                    gas_used: U256::from(gas_limit),
                    error: halted.then(|| "out of gas".into()),
                    logs: res
                        .result
                        .logs()
                        .iter()
                        .enumerate()
                        .map(|(index, log)| CallLogFrame {
                            address: Some(log.address),
                            topics: Some(log.topics().to_vec()),
                            data: Some(log.data.data.clone()),
                            position: Some(0),
                            index: Some(index as u64),
                        })
                        .collect(),
                    // Only recorded roots currently capture the execution spec.
                    execution_gas_used: (!halted)
                        .then(|| U256::from(res.result.gas().block_regular_gas_used())),
                    state_gas_used: (!halted)
                        .then(|| U256::from(res.result.gas().block_state_gas_used())),
                    gas_refund: (!halted).then(|| U256::from(res.result.gas().final_refunded())),
                    ..Default::default()
                };
                assert_eq!(frame, serde_json::to_value(expected).unwrap());
                inspector.fuse().unwrap();
            }
        }
    }
}

#[test]
fn missing_root_other_halt_is_not_out_of_gas() {
    let mut inspector =
        DebugInspector::new(GethDebugTracingOptions::call_tracer(CallConfig::default())).unwrap();
    let mut evm = Context::mainnet().build_mainnet().with_inspector(&mut inspector);
    let mut res = evm.inspect_tx(TxEnv::default()).unwrap();
    res.result = ExecutionResult::Halt {
        reason: HaltReason::InvalidFEOpcode,
        gas: *res.result.gas(),
        logs: Vec::new(),
    };
    let (ctx, inspector) = evm.ctx_inspector();
    inspector.fuse().unwrap();
    let tx = ctx.tx().clone();
    let block = ctx.block().clone();
    let trace = inspector.get_result(None, &tx, &block, &res, ctx.db_mut()).unwrap();
    let GethTrace::CallTracer(frame) = trace else { panic!("expected call trace") };
    // An unknown missing frame must not be mislabeled as a runtime OOG.
    assert_eq!(frame.error, None);
}
