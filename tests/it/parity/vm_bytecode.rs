use alloy_primitives::{hex, map::HashSet, Address, Bytes, U256};
use alloy_rpc_types_trace::parity::{TraceResults, TraceType};
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

fn trace(db: CacheDB<EmptyDB>, tx: TxEnv) -> TraceResults {
    let types = HashSet::from_iter([TraceType::VmTrace]);
    let mut evm = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::PRAGUE))
        .with_db(db)
        .build_mainnet_with_inspector(TracingInspector::new(
            TracingInspectorConfig::from_parity_config(&types),
        ));
    let result = evm.inspect_tx(tx).unwrap();
    let builder = evm.inspector.into_parity_builder().unwrap();
    let direct = builder.vm_trace();
    let traces = builder
        .into_trace_results_with_state(&result, &types, &evm.ctx.journaled_state.database)
        .unwrap();
    assert_eq!(Some(direct), traces.vm_trace);
    traces
}

#[test]
fn constructor_bytecode() {
    let initcode = hex!("60016000526001601ff3");
    let traces = trace(
        CacheDB::default(),
        TxEnv::builder().create().data(initcode.into()).gas_limit(100_000).build_fill(),
    );
    assert_eq!(traces.output.as_ref(), [0x01]);
    let vm = traces.vm_trace.unwrap();
    assert_eq!(vm.code.as_ref(), initcode);
    assert_eq!(vm.ops.last().unwrap().op.as_deref(), Some("RETURN"));
}

#[test]
fn nested_creation_bytecode() {
    for create2 in [false, true] {
        // Store initcode that returns STOP at offset 24, then CREATE or CREATE2 it.
        let initcode = hex!("60005f5360015ff3");
        let mut code = hex!("6760005f5360015ff35f52").to_vec();
        if create2 {
            code.push(0x5f);
        }
        code.extend(hex!("600860185f"));
        code.push(if create2 { 0xf5 } else { 0xf0 });
        code.push(0x00);
        let target = Address::with_last_byte(0x42);
        let mut db = CacheDB::default();
        db.insert_account_info(
            target,
            AccountInfo::default().with_code(Bytecode::new_raw(code.clone().into())),
        );
        let traces = trace(db, TxEnv::builder().to(target).gas_limit(200_000).build_fill());
        let vm = traces.vm_trace.unwrap();
        assert_eq!(vm.code.as_ref(), code);
        let child = vm.ops.iter().find_map(|op| op.sub.as_ref()).unwrap();
        assert_eq!(child.code.as_ref(), initcode);
        assert_eq!(child.ops.last().unwrap().op.as_deref(), Some("RETURN"));
    }
}

#[test]
fn authorization_resolves_executed_bytecode() {
    let authority = Address::with_last_byte(0x42);
    let old_delegate = Address::with_last_byte(0x43);
    let delegate = Address::with_last_byte(0x44);
    for before in [Bytecode::default(), Bytecode::new_eip7702(old_delegate)] {
        for (delegate, code) in [
            (delegate, Bytes::from_static(&hex!("602a5f5260205ff3"))),
            (delegate, Bytes::from_static(&hex!("5f5ffd"))),
            (Address::ZERO, Bytes::new()),
        ] {
            let mut db = CacheDB::default();
            db.insert_account_info(authority, AccountInfo::default().with_code(before.clone()));
            db.insert_account_info(
                old_delegate,
                AccountInfo::default().with_code(Bytecode::new_raw(hex!("60015000").into())),
            );
            if !delegate.is_zero() {
                db.insert_account_info(
                    delegate,
                    AccountInfo::default().with_code(Bytecode::new_raw(code.clone())),
                );
            }
            let traces = trace(
                db,
                TxEnv::builder()
                    .to(authority)
                    .gas_limit(100_000)
                    .authorization_list_recovered(vec![RecoveredAuthorization::new_unchecked(
                        Authorization { chain_id: U256::from(1), address: delegate, nonce: 0 },
                        RecoveredAuthority::Valid(authority),
                    )])
                    .build_fill(),
            );
            let vm = traces.vm_trace.unwrap();
            assert_eq!(vm.code, code);
            if code.is_empty() {
                assert!(vm.ops.is_empty());
            } else {
                assert!(!vm.ops.is_empty());
            }
        }
    }
}

#[test]
fn callcode_and_delegatecall_bytecode() {
    let target = Address::with_last_byte(0x42);
    let child = Address::with_last_byte(0x43);
    let child_code = hex!("602a5f5260205ff3");
    for code in [hex!("5f5f5f5f5f604361fffff200").to_vec(), hex!("5f5f5f5f604361fffff400").to_vec()]
    {
        let mut db = CacheDB::default();
        db.insert_account_info(
            target,
            AccountInfo::default().with_code(Bytecode::new_raw(code.clone().into())),
        );
        db.insert_account_info(
            child,
            AccountInfo::default().with_code(Bytecode::new_raw(child_code.into())),
        );
        let traces = trace(db, TxEnv::builder().to(target).gas_limit(100_000).build_fill());
        let vm = traces.vm_trace.unwrap();
        assert_eq!(vm.code.as_ref(), code);
        let sub = vm.ops.iter().find_map(|op| op.sub.as_ref()).unwrap();
        assert_eq!(sub.code.as_ref(), child_code);
    }
}

#[test]
fn bytecode_recording_is_opt_in() {
    let code = hex!("60015000");
    for config in [
        TracingInspectorConfig::default_geth(),
        TracingInspectorConfig::default_parity(),
        TracingInspectorConfig::parity_statediff(),
    ] {
        let inspector = super::inspect_code(&code, &[], SpecId::PRAGUE, config);
        assert!(inspector
            .traces()
            .unwrap()
            .nodes()
            .iter()
            .all(|node| node.trace.bytecode.is_none()));
    }
    let mut config = TracingInspectorConfig::default_geth();
    config.merge(TracingInspectorConfig::parity_vm_trace());
    let inspector = super::inspect_code(&code, &[], SpecId::PRAGUE, config);
    assert_eq!(
        inspector.traces().unwrap().nodes()[0].trace.bytecode.as_ref().unwrap().as_ref(),
        code
    );
}

#[test]
fn bytecode_recording_reuses_original_buffer() {
    // Keep a full contract-sized buffer alive so copying it is observable by pointer identity.
    let mut bytes = vec![0x5b; 24_576];
    bytes[0] = 0x00;
    let code = Bytecode::new_raw(bytes.into());
    let target = Address::with_last_byte(0x42);
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(target, AccountInfo::default().with_code(code.clone()));
    let mut evm = Context::mainnet().with_db(db).build_mainnet_with_inspector(
        TracingInspector::new(TracingInspectorConfig::parity_vm_trace()),
    );
    let result =
        evm.inspect_tx(TxEnv::builder().to(target).gas_limit(100_000).build_fill()).unwrap();
    assert!(result.result.is_success(), "{result:#?}");

    let recorded = evm.inspector.traces().unwrap().nodes()[0].trace.bytecode.as_ref().unwrap();
    assert_eq!(recorded.as_ptr(), code.original_byte_slice().as_ptr());
    assert_eq!(recorded.len(), code.len());

    let vm_trace = evm.inspector.into_parity_builder().unwrap().vm_trace();
    assert_eq!(vm_trace.code.as_ptr(), code.original_byte_slice().as_ptr());
    assert_eq!(vm_trace.code.len(), code.len());
}
