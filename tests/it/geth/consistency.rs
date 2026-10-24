use super::*;
use alloy_rpc_types_trace::parity::Action;
use serde_json::{json, Value};

fn trace(options: Value, code: Bytes, create: bool) -> (GethTrace, u64) {
    let target = Address::repeat_byte(0xaa);
    let caller = Address::repeat_byte(0xbb);
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(
        target,
        AccountInfo::default().with_code(Bytecode::new_raw(code.clone())),
    );
    let inspector = DebugInspector::new(serde_json::from_value(options).unwrap()).unwrap();
    let context = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::CANCUN))
        .with_db(db);
    let mut evm = context.build_mainnet_with_inspector(inspector);
    let tx = TxEnv {
        caller,
        gas_limit: 1_000_000,
        kind: if create { TxKind::Create } else { TxKind::Call(target) },
        data: if create { code } else { Bytes::new() },
        ..Default::default()
    };
    let result = evm.inspect_tx(tx.clone()).unwrap();
    let (ctx, inspector) = evm.ctx_inspector();
    let block = ctx.block().clone();
    let trace = inspector.get_result(None, &tx, &block, &result, ctx.db_mut()).unwrap();
    (trace, result.result.tx_gas_used())
}

#[test]
fn mux_config_matches_standalone() {
    for tracer in ["4byteTracer", "noopTracer", "stateGasTracer"] {
        for config in [Value::Null, json!({}), json!({"onlyTopCall":true}), json!("ignored")] {
            let standalone = json!({"tracer":tracer, "tracerConfig":config});
            let mux = json!({"tracer":"muxTracer", "tracerConfig":{tracer:config}});
            assert!(DebugInspector::new(serde_json::from_value(standalone).unwrap()).is_ok());
            assert!(DebugInspector::new(serde_json::from_value(mux).unwrap()).is_ok());
        }
    }
    for tracer in ["callTracer", "prestateTracer", "flatCallTracer"] {
        let options = |config| json!({"tracer":"muxTracer", "tracerConfig":{tracer:config}});
        assert_eq!(
            trace(options(Value::Null), hex!("600160005500").into(), false).0,
            trace(options(json!({})), hex!("600160005500").into(), false).0,
        );
        assert!(DebugInspector::new(serde_json::from_value(options(json!("invalid"))).unwrap())
            .is_err());
    }
}

#[test]
fn mux_and_standalone_roots_match() {
    for create in [false, true] {
        for code in [hex!("600160005200").as_slice(), &hex!("60006000fd")] {
            for tracer in ["callTracer", "flatCallTracer"] {
                let (standalone, gas_used) = trace(json!({"tracer":tracer}), code.into(), create);
                let (mux, _) = trace(
                    json!({"tracer":"muxTracer", "tracerConfig":{tracer:{}}}),
                    code.into(),
                    create,
                );
                let mux = serde_json::to_value(mux).unwrap();
                assert_eq!(mux[tracer], serde_json::to_value(&standalone).unwrap());
                match standalone {
                    GethTrace::CallTracer(root) => {
                        assert_eq!(root.from, Address::repeat_byte(0xbb));
                        assert_eq!(root.gas, U256::from(1_000_000));
                        assert_eq!(root.gas_used, U256::from(gas_used));
                    }
                    GethTrace::FlatCallTracer(traces) => {
                        let root = &traces[0].trace;
                        match &root.action {
                            Action::Call(action) => assert_eq!(action.gas, 1_000_000),
                            Action::Create(action) => assert_eq!(action.gas, 1_000_000),
                            action => panic!("unexpected root {action:?}"),
                        }
                        if let Some(result) = &root.result {
                            assert_eq!(result.gas_used(), gas_used);
                        }
                    }
                    other => panic!("unexpected trace {other:?}"),
                }
            }
        }
    }
}
