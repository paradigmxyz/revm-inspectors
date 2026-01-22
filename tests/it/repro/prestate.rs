//! Prestate tracer reproduction tests.

use super::ReproContext;
use alloy_rpc_types_trace::geth::{DiffMode, PreStateConfig, PreStateFrame};
use revm::{Context, InspectEvm, MainBuilder, MainContext};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

const TX_SELFDESTRUCT: &str = include_str!("../../../testdata/repro/tx-selfdestruct.json");

#[test]
fn test_prestate_tracer_selfdestruct() {
    let ctx = ReproContext::load(TX_SELFDESTRUCT);
    let prestate_config = PreStateConfig::default();

    let mut inspector =
        TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));

    let db = ctx.db.clone();
    let mut evm = Context::mainnet()
        .with_db(ctx.db.clone())
        .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
        .build_mainnet()
        .with_inspector(&mut inspector);

    let res = evm.inspect_tx(ctx.tx_env()).expect("tx should execute");
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
            assert!(prestate.0.contains_key(&ctx.fixture.transaction.from));
            if let Some(to) = ctx.fixture.transaction.to {
                assert!(prestate.0.contains_key(&to));
            }
        }
        PreStateFrame::Diff(_) => panic!("Expected default prestate, got diff"),
    }
}

#[test]
fn test_prestate_tracer_selfdestruct_diff_mode() {
    let ctx = ReproContext::load(TX_SELFDESTRUCT);
    let prestate_config = PreStateConfig { diff_mode: Some(true), ..Default::default() };

    let mut inspector =
        TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));

    let db = ctx.db.clone();
    let mut evm = Context::mainnet()
        .with_db(ctx.db.clone())
        .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
        .build_mainnet()
        .with_inspector(&mut inspector);

    let res = evm.inspect_tx(ctx.tx_env()).expect("tx should execute");
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
            assert!(pre.contains_key(&ctx.fixture.transaction.from));
            // The sender should also be in post with updated state
            assert!(post.contains_key(&ctx.fixture.transaction.from));
        }
        PreStateFrame::Default(_) => panic!("Expected diff mode, got default"),
    }
}
