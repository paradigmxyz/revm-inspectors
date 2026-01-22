//! Prestate tracer reproduction tests.
//!
//! These tests demonstrate replaying mainnet transactions using prestate tracer output.

use super::ReproContext;
use alloy_primitives::{address, hex, Bytes};
use alloy_rpc_types_trace::geth::{DiffMode, PreStateConfig, PreStateFrame};
use revm::{
    context::TxEnv, context_interface::TransactTo, primitives::hardfork::SpecId, Context,
    InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

/// Raw prestate tracer RPC response for mainnet tx:
/// 0x391f4b6a382d3bcc3120adc2ea8c62003e604e487d97281129156fd284a1a89d
/// Block: 19660754 (Cancun)
const TX_SELFDESTRUCT_PRESTATE: &str = include_str!("../../../testdata/repro/tx-selfdestruct.json");

/// Transaction details (extracted from etherscan/RPC):
/// - from: 0xa7fb5ca286fc3fd67525629048a4de3ba24cba2e
/// - to: 0xc77ad0a71008d7094a62cfbd250a2eb2afdf2776
/// - input: 0xf3fef3a3...  (withdraw function call)
/// - nonce: 0x39af8 (236280)
/// - gas: 0x249f0 (150000)
fn tx_selfdestruct_env() -> TxEnv {
    TxEnv {
        caller: address!("a7fb5ca286fc3fd67525629048a4de3ba24cba2e"),
        kind: TransactTo::Call(address!("c77ad0a71008d7094a62cfbd250a2eb2afdf2776")),
        data: Bytes::from_static(&hex!(
            "f3fef3a3000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec700000000000000000000000000000000000000000000000000000000000f6b64"
        )),
        nonce: 236280,
        gas_limit: 150000,
        ..Default::default()
    }
}

#[test]
fn test_prestate_tracer_selfdestruct() {
    let ctx =
        ReproContext::from_prestate_response(TX_SELFDESTRUCT_PRESTATE).with_block_number(19660754);
    let tx_env = tx_selfdestruct_env();
    let prestate_config = PreStateConfig::default();

    let mut inspector =
        TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));

    let db = ctx.db.clone();
    let mut evm = Context::mainnet()
        .with_db(ctx.db.clone())
        .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
        .build_mainnet()
        .with_inspector(&mut inspector);

    let res = evm.inspect_tx(tx_env.clone()).expect("tx should execute");
    assert!(res.result.is_success(), "tx failed: {:?}", res.result);

    // Get the prestate trace
    let frame = inspector
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_prestate_traces(&res, &prestate_config, db)
        .unwrap();

    // Verify the trace contains expected accounts
    match frame {
        PreStateFrame::Default(prestate) => {
            // The prestate should contain the accounts that were accessed
            assert!(prestate.0.contains_key(&tx_env.caller));
            if let TransactTo::Call(to) = tx_env.kind {
                assert!(prestate.0.contains_key(&to));
            }
        }
        PreStateFrame::Diff(_) => panic!("Expected default prestate, got diff"),
    }
}

#[test]
fn test_prestate_tracer_selfdestruct_diff_mode() {
    let ctx =
        ReproContext::from_prestate_response(TX_SELFDESTRUCT_PRESTATE).with_spec_id(SpecId::CANCUN);
    let tx_env = tx_selfdestruct_env();
    let prestate_config = PreStateConfig { diff_mode: Some(true), ..Default::default() };

    let mut inspector =
        TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));

    let db = ctx.db.clone();
    let mut evm = Context::mainnet()
        .with_db(ctx.db.clone())
        .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
        .build_mainnet()
        .with_inspector(&mut inspector);

    let res = evm.inspect_tx(tx_env.clone()).expect("tx should execute");
    assert!(res.result.is_success(), "tx failed: {:?}", res.result);

    let frame = inspector
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_prestate_traces(&res, &prestate_config, db)
        .unwrap();

    // In diff mode, we should see the changes between pre and post state
    match frame {
        PreStateFrame::Diff(DiffMode { pre, post }) => {
            // The sender should be in pre (balance changed)
            assert!(pre.contains_key(&tx_env.caller));
            // The sender should also be in post with updated state
            assert!(post.contains_key(&tx_env.caller));
        }
        PreStateFrame::Default(_) => panic!("Expected diff mode, got default"),
    }
}
