use alloy_primitives::{hex, Address, U256};
use alloy_rpc_types_trace::geth::{PreStateConfig, PreStateFrame};
use revm::{
    context::TxEnv,
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn created_account_selfdestruct_across_cancun() {
    for spec in [SpecId::SHANGHAI, SpecId::CANCUN, SpecId::AMSTERDAM] {
        for prefunded in [false, true] {
            let caller = Address::with_last_byte(0x42);
            let target = caller.create(0);
            let mut db = CacheDB::<EmptyDB>::default();
            if prefunded {
                db.insert_account_info(
                    target,
                    AccountInfo { balance: U256::from(69), ..Default::default() },
                );
            }
            let config = PreStateConfig { diff_mode: Some(true), ..Default::default() };
            let mut evm = Context::mainnet()
                .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(spec))
                .with_db(db)
                .build_mainnet_with_inspector(TracingInspector::new(
                    TracingInspectorConfig::from_geth_prestate_config(&config),
                ));
            let result = evm
                .inspect_tx(
                    TxEnv::builder()
                        .caller(caller)
                        .create()
                        // Write storage before selfdestructing to the caller.
                        .data(hex!("600160005533ff").into())
                        .gas_limit(1_000_000)
                        .build_fill(),
                )
                .unwrap();
            assert!(
                result.result.is_success(),
                "{spec:?}, prefunded: {prefunded}: {:?}",
                result.result
            );
            let PreStateFrame::Diff(diff) = evm
                .inspector
                .geth_builder()
                .geth_prestate_traces(&result, &config, &evm.ctx.journaled_state.database)
                .unwrap()
            else {
                panic!("expected a prestate diff");
            };
            assert!(!diff.post.contains_key(&target), "{spec:?}, prefunded: {prefunded}");
            if prefunded {
                let pre = &diff.pre[&target];
                assert_eq!(pre.balance, Some(U256::from(69)));
                assert!(pre.storage.is_empty());
            } else {
                assert!(!diff.pre.contains_key(&target), "{spec:?}");
            }
        }
    }
}
