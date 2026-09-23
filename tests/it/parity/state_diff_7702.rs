use alloy_primitives::{hex, map::HashSet, Address, U256, U64};
use alloy_rpc_types_trace::parity::{Delta, TraceType};
use revm::{
    bytecode::Bytecode,
    context::TxEnv,
    context_interface::transaction::{Authorization, RecoveredAuthority, RecoveredAuthorization},
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId,
    state::AccountInfo,
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn authorization_code_changes() {
    let old_delegate = Address::with_last_byte(0x41);
    let new_delegate = Address::with_last_byte(0x42);
    for (before, delegate) in [
        (Bytecode::default(), new_delegate),
        (Bytecode::new_eip7702(old_delegate), new_delegate),
        (Bytecode::new_eip7702(old_delegate), Address::ZERO),
        (Bytecode::new_eip7702(new_delegate), new_delegate),
    ] {
        // Authorization changes persist even when transaction execution reverts.
        for revert in [false, true] {
            let authority = Address::with_last_byte(0x43);
            let target = Address::with_last_byte(0x44);
            let mut db = CacheDB::<EmptyDB>::default();
            db.insert_account_info(
                authority,
                AccountInfo { nonce: 1, ..Default::default() }.with_code(before.clone()),
            );
            db.insert_account_info(
                target,
                AccountInfo::default().with_code(Bytecode::new_raw(
                    if revert { hex!("5f5ffd").to_vec() } else { vec![0x00] }.into(),
                )),
            );
            let mut evm = Context::mainnet()
                .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::PRAGUE))
                .with_db(db)
                .build_mainnet_with_inspector(TracingInspector::new(
                    TracingInspectorConfig::default_parity(),
                ));
            let result = evm
                .inspect_tx(
                    TxEnv::builder()
                        .to(target)
                        .gas_limit(100_000)
                        .authorization_list_recovered(vec![RecoveredAuthorization::new_unchecked(
                            Authorization { chain_id: U256::from(1), address: delegate, nonce: 1 },
                            RecoveredAuthority::Valid(authority),
                        )])
                        .build_fill(),
                )
                .unwrap();
            assert_eq!(result.result.is_success(), !revert);
            assert!(!result.state[&authority].is_created());
            let after = if delegate.is_zero() {
                Bytecode::default()
            } else {
                Bytecode::new_eip7702(delegate)
            };
            assert_eq!(result.state[&authority].info.code_hash, after.hash_slow());
            let traces = evm
                .inspector
                .into_parity_builder()
                .unwrap()
                .into_trace_results_with_state(
                    &result,
                    &HashSet::from_iter([TraceType::StateDiff]),
                    &evm.ctx.journaled_state.database,
                )
                .unwrap();
            let diff = &traces.state_diff.unwrap()[&authority];
            let expected = if before == after {
                Delta::Unchanged
            } else {
                Delta::changed(before.original_bytes(), after.original_bytes())
            };
            assert_eq!(diff.code, expected, "delegate={delegate}, revert={revert}");
            assert_eq!(diff.nonce, Delta::changed(U64::from(1), U64::from(2)));
            assert_eq!(diff.balance, Delta::Unchanged);
        }
    }
}
