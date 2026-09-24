use alloy_primitives::{map::HashSet, Address, U256, U64};
use alloy_rpc_types_trace::parity::{Delta, TraceType};
use revm::{
    context::TxEnv,
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn transfer_state_diff_depends_on_recipient_existence() {
    let caller = Address::with_last_byte(0x42);
    let recipient = Address::with_last_byte(0x44);
    for initial_balance in [None, Some(U256::ZERO), Some(U256::from(5))] {
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(
            caller,
            AccountInfo { balance: U256::from(100), ..Default::default() },
        );
        if let Some(balance) = initial_balance {
            db.insert_account_info(
                recipient,
                AccountInfo { balance, nonce: 1, ..Default::default() },
            );
        }
        let mut evm = Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::PRAGUE))
            .with_db(db)
            .build_mainnet_with_inspector(TracingInspector::new(
                TracingInspectorConfig::default_parity(),
            ));
        let result = evm
            .inspect_tx(
                TxEnv::builder()
                    .caller(caller)
                    .to(recipient)
                    .value(U256::from(7))
                    .gas_limit(100_000)
                    .build_fill(),
            )
            .unwrap();
        assert!(result.result.is_success());
        let traces = evm
            .inspector
            .into_parity_builder()
            .into_trace_results_with_state(
                &result,
                &HashSet::from_iter([TraceType::StateDiff]),
                &evm.ctx.journaled_state.database,
            )
            .unwrap();
        let diff = &traces.state_diff.unwrap()[&recipient];
        if let Some(balance) = initial_balance {
            assert_eq!(diff.balance, Delta::changed(balance, balance + U256::from(7)));
            assert_eq!(diff.nonce, Delta::Unchanged);
            assert_eq!(diff.code, Delta::Unchanged);
        } else {
            assert_eq!(diff.balance, Delta::Added(U256::from(7)));
            assert_eq!(diff.nonce, Delta::Added(U64::ZERO));
            assert_eq!(diff.code, Delta::Added(Default::default()));
            let json = serde_json::to_value(diff).unwrap();
            assert_eq!(json["balance"], serde_json::json!({"+": "0x7"}));
            assert_eq!(json["nonce"], serde_json::json!({"+": "0x0"}));
            assert_eq!(json["code"], serde_json::json!({"+": "0x"}));
        }
        assert!(diff.storage.is_empty());
    }
}

#[test]
fn zero_value_transfer_to_absent_account_has_no_state_diff() {
    let recipient = Address::with_last_byte(0x44);
    let mut evm = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::PRAGUE))
        .with_db(CacheDB::<EmptyDB>::default())
        .build_mainnet_with_inspector(TracingInspector::new(
            TracingInspectorConfig::default_parity(),
        ));
    let result =
        evm.inspect_tx(TxEnv::builder().to(recipient).gas_limit(100_000).build_fill()).unwrap();
    assert!(result.result.is_success());
    let traces = evm
        .inspector
        .into_parity_builder()
        .into_trace_results_with_state(
            &result,
            &HashSet::from_iter([TraceType::StateDiff]),
            &evm.ctx.journaled_state.database,
        )
        .unwrap();
    assert!(!traces.state_diff.unwrap().contains_key(&recipient));
}
