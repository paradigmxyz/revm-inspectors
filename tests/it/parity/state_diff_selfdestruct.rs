use alloy_primitives::{hex, map::HashSet, Address, B256, U256, U64};
use alloy_rpc_types_trace::parity::{Delta, TraceType};
use revm::{
    bytecode::Bytecode,
    context::TxEnv,
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn existing_account_selfdestruct_across_cancun() {
    for spec in [SpecId::SHANGHAI, SpecId::CANCUN] {
        for revert in [false, true] {
            let target = Address::with_last_byte(0x42);
            let parent = Address::with_last_byte(0x43);
            // Read an unchanged slot, overwrite a nonzero slot, write a new slot, read zero,
            // then selfdestruct.
            let code = Bytecode::new_raw(hex!("60005450600960015560036002556003545033ff").into());
            let mut db = CacheDB::<EmptyDB>::default();
            db.insert_account_info(
                target,
                AccountInfo { balance: U256::from(69), nonce: 1, ..Default::default() }
                    .with_code(code.clone()),
            );
            db.insert_account_storage(target, U256::ZERO, U256::from(7)).unwrap();
            db.insert_account_storage(target, U256::from(1), U256::from(8)).unwrap();
            db.insert_account_info(
                parent,
                AccountInfo::default().with_code(Bytecode::new_raw(
                    hex!("60006000600060006000604261fffff15060006000fd").into(),
                )),
            );
            let mut evm = Context::mainnet()
                .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(spec))
                .with_db(db)
                .build_mainnet_with_inspector(TracingInspector::new(
                    TracingInspectorConfig::default_parity(),
                ));
            let result = evm
                .inspect_tx(
                    TxEnv::builder()
                        .to(if revert { parent } else { target })
                        .gas_limit(200_000)
                        .build_fill(),
                )
                .unwrap();
            assert_eq!(result.result.is_success(), !revert);
            assert_eq!(result.state[&target].is_selfdestructed(), spec < SpecId::CANCUN && !revert);
            let traces = evm
                .inspector
                .into_parity_builder()
                .into_trace_results_with_state(
                    &result,
                    &HashSet::from_iter([TraceType::StateDiff]),
                    &evm.ctx.journaled_state.database,
                )
                .unwrap();
            let state_diff = traces.state_diff.unwrap();
            if revert {
                assert!(!state_diff.contains_key(&target));
                continue;
            }
            let diff = &state_diff[&target];
            if spec < SpecId::CANCUN {
                assert_eq!(diff.balance, Delta::Removed(U256::from(69)));
                assert_eq!(diff.nonce, Delta::Removed(U64::from(1)));
                assert_eq!(diff.code, Delta::Removed(code.original_bytes()));
                // The removed account implies its storage is wiped; no slot is listed.
                assert!(diff.storage.is_empty());
                let json = serde_json::to_value(diff).unwrap();
                assert_eq!(json["nonce"], serde_json::json!({"-": "0x1"}));
                assert_eq!(json["storage"], serde_json::json!({}));
            } else {
                assert_eq!(diff.balance, Delta::changed(U256::from(69), U256::ZERO));
                assert_eq!(diff.nonce, Delta::Unchanged);
                assert_eq!(diff.code, Delta::Unchanged);
                assert_eq!(diff.storage.len(), 2);
                assert_eq!(
                    diff.storage[&B256::with_last_byte(1)],
                    Delta::changed(U256::from(8).into(), U256::from(9).into())
                );
                assert_eq!(
                    diff.storage[&B256::with_last_byte(2)],
                    Delta::changed(U256::ZERO.into(), U256::from(3).into())
                );
            }
        }
    }
}

#[test]
fn created_account_selfdestruct_across_cancun() {
    for spec in [SpecId::SHANGHAI, SpecId::CANCUN] {
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
            let mut evm = Context::mainnet()
                .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(spec))
                .with_db(db)
                .build_mainnet_with_inspector(TracingInspector::new(
                    TracingInspectorConfig::default_parity(),
                ));
            let result = evm
                .inspect_tx(
                    TxEnv::builder()
                        .caller(caller)
                        .create()
                        .data(hex!("33ff").into())
                        .gas_limit(100_000)
                        .build_fill(),
                )
                .unwrap();
            assert!(result.result.is_success());
            assert!(result.state[&target].is_created());
            assert!(result.state[&target].is_selfdestructed());
            let traces = evm
                .inspector
                .into_parity_builder()
                .into_trace_results_with_state(
                    &result,
                    &HashSet::from_iter([TraceType::StateDiff]),
                    &evm.ctx.journaled_state.database,
                )
                .unwrap();
            let diff = traces.state_diff.unwrap();
            if prefunded {
                let diff = &diff[&target];
                assert_eq!(diff.balance, Delta::Removed(U256::from(69)));
                assert_eq!(diff.nonce, Delta::Removed(U64::ZERO));
                assert_eq!(diff.code, Delta::Removed(Default::default()));
                assert!(diff.storage.is_empty());
            } else {
                assert!(!diff.contains_key(&target));
            }
        }
    }
}

#[test]
fn deleted_account_diff_does_not_depend_on_accessed_slots() {
    // Before Cancun, the same account with the same storage is deleted by code that touches
    // its slots and by code that does not; the deletion entry must be identical.
    let target = Address::with_last_byte(0x42);
    let diffs = [&hex!("60005450600960015533ff")[..], &hex!("33ff")[..]].map(|code| {
        let code = Bytecode::new_raw(code.to_vec().into());
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(
            target,
            AccountInfo { balance: U256::from(69), nonce: 1, ..Default::default() }.with_code(code),
        );
        db.insert_account_storage(target, U256::ZERO, U256::from(7)).unwrap();
        db.insert_account_storage(target, U256::from(1), U256::from(8)).unwrap();
        let mut evm = Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::SHANGHAI))
            .with_db(db)
            .build_mainnet_with_inspector(TracingInspector::new(
                TracingInspectorConfig::default_parity(),
            ));
        let result =
            evm.inspect_tx(TxEnv::builder().to(target).gas_limit(200_000).build_fill()).unwrap();
        assert!(result.state[&target].is_selfdestructed());
        let traces = evm
            .inspector
            .into_parity_builder()
            .into_trace_results_with_state(
                &result,
                &HashSet::from_iter([TraceType::StateDiff]),
                &evm.ctx.journaled_state.database,
            )
            .unwrap();
        let diff = traces.state_diff.unwrap()[&target].clone();
        assert!(diff.storage.is_empty());
        diff
    });
    assert_eq!(diffs[0].balance, diffs[1].balance);
    assert_eq!(diffs[0].nonce, diffs[1].nonce);
    assert_eq!(diffs[0].storage, diffs[1].storage);
}
