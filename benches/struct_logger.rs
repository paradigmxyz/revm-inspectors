//! End-to-end opcode logger benchmarks for a call-heavy contract.
//!
//! Split in two halves so that regressions can be attributed:
//! * `record` measures the inspector hooks, i.e. building the `CallTraceStep` vectors.
//! * `build` measures turning already recorded steps into geth `StructLog`s.

use alloy_primitives::{Address, Bytes, U256};
use alloy_rpc_types_trace::geth::GethDefaultTracingOptions;
use criterion::{criterion_group, criterion_main, Criterion};
use revm::{
    context::TxEnv,
    context_interface::{result::ExecutionResult, TransactTo},
    database::CacheDB,
    database_interface::EmptyDB,
    primitives::hardfork::SpecId,
    state::{AccountInfo, Bytecode},
    Context, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{
    geth::GethTraceBuilder, types::CallTraceNode, TracingInspector, TracingInspectorConfig,
};
use std::hint::black_box;

/// Number of subcalls the caller contract performs.
const CALL_COUNT: usize = 300;
/// Number of arithmetic sequences the callee runs per invocation.
const HELPER_OPS: usize = 80;
/// Gas forwarded to each subcall.
const CALL_GAS: u16 = 5_000;

const CONTRACT_ADDRESS: Address = Address::repeat_byte(0x01);
const HELPER_ADDRESS: Address = Address::repeat_byte(0x02);
const MEMORY_WORD: [u8; 32] = [0x11; 32];

/// Caller contract: memory, storage and arithmetic work interleaved with static calls.
fn caller_contract() -> Bytes {
    let mut code = Vec::with_capacity(CALL_COUNT * 96);
    for _ in 0..CALL_COUNT {
        // PUSH1 1 PUSH1 2 ADD POP
        code.extend_from_slice(&[0x60, 0x01, 0x60, 0x02, 0x01, 0x50]);
        // PUSH32 word PUSH1 0 MSTORE
        code.push(0x7f);
        code.extend_from_slice(&MEMORY_WORD);
        code.extend_from_slice(&[0x60, 0x00, 0x52]);
        // PUSH1 0 SLOAD POP
        code.extend_from_slice(&[0x60, 0x00, 0x54, 0x50]);
        // PUSH1 0x2a PUSH1 0 SSTORE
        code.extend_from_slice(&[0x60, 0x2a, 0x60, 0x00, 0x55]);
        // STATICCALL(gas, helper, 0, 0, 0, 0) POP
        code.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x60, 0x00]);
        code.push(0x73);
        code.extend_from_slice(HELPER_ADDRESS.as_slice());
        code.extend_from_slice(&[0x61, (CALL_GAS >> 8) as u8, CALL_GAS as u8, 0xfa, 0x50]);
    }
    code.push(0x00);
    code.into()
}

/// Callee contract: pure arithmetic plus a warm storage read.
fn helper_contract() -> Bytes {
    let mut code = Vec::with_capacity((HELPER_OPS * 6) + 8);
    // PUSH1 0 SLOAD POP
    code.extend_from_slice(&[0x60, 0x00, 0x54, 0x50]);
    for _ in 0..HELPER_OPS {
        // PUSH1 1 PUSH1 2 ADD POP
        code.extend_from_slice(&[0x60, 0x01, 0x60, 0x02, 0x01, 0x50]);
    }
    code.push(0x00);
    code.into()
}

fn database() -> CacheDB<EmptyDB> {
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        Address::ZERO,
        AccountInfo { balance: U256::from(1e18), ..Default::default() },
    );
    db.insert_account_info(
        CONTRACT_ADDRESS,
        AccountInfo { code: Some(Bytecode::new_legacy(caller_contract())), ..Default::default() },
    );
    db.insert_account_info(
        HELPER_ADDRESS,
        AccountInfo { code: Some(Bytecode::new_legacy(helper_contract())), ..Default::default() },
    );
    db
}

fn tx_env() -> TxEnv {
    TxEnv {
        gas_price: 0,
        gas_limit: 30_000_000,
        gas_priority_fee: None,
        kind: TransactTo::Call(CONTRACT_ADDRESS),
        ..Default::default()
    }
}

/// Runs the transaction with the given config and returns the recorded trace nodes.
fn record(
    db: CacheDB<EmptyDB>,
    config: TracingInspectorConfig,
) -> (Vec<CallTraceNode>, u64, Bytes) {
    let inspector = TracingInspector::new(config);
    let mut evm = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::PRAGUE)
        .with_db(db)
        .build_mainnet_with_inspector(inspector);

    let res = evm.inspect_tx(tx_env()).expect("transaction should execute");
    assert!(matches!(res.result, ExecutionResult::Success { .. }), "{:?}", res.result);
    let gas_used = res.result.tx_gas_used();
    let output = res.result.output().cloned().unwrap_or_default();

    (evm.inspector.into_traces().into_nodes(), gas_used, output)
}

/// The geth options matching a `TracingInspectorConfig`, so recording and building agree.
fn geth_options(disabled: bool, full: bool) -> GethDefaultTracingOptions {
    let mut opts =
        GethDefaultTracingOptions::default().with_enable_memory(full).with_enable_return_data(full);
    if disabled {
        opts = opts.disable_stack().disable_storage();
    }
    opts
}

fn struct_logger_benches(c: &mut Criterion) {
    let db = database();

    // stack + storage on, memory + return data off: the geth default.
    let default_opts = geth_options(false, false);
    // everything the struct logger can capture.
    let full_opts = geth_options(false, true);
    // `disableStack` + `disableStorage`, the cheapest struct log response.
    let disabled_opts = geth_options(true, false);

    let configs = [("default", default_opts), ("full", full_opts), ("disabled", disabled_opts)];

    let mut group = c.benchmark_group("struct_logger");

    for (name, opts) in configs {
        let config = TracingInspectorConfig::from_geth_config(&opts);
        group.bench_function(format!("record/{name}"), |b| {
            b.iter(|| black_box(record(db.clone(), config)));
        });
    }

    for (name, opts) in configs {
        let config = TracingInspectorConfig::from_geth_config(&opts);
        let (nodes, gas_used, output) = record(db.clone(), config);
        group.bench_function(format!("build/{name}"), |b| {
            b.iter(|| {
                black_box(
                    GethTraceBuilder::new_borrowed(&nodes)
                        .with_spec_id(SpecId::PRAGUE)
                        .geth_traces(gas_used, output.clone(), opts),
                )
            });
        });
    }

    group.finish();
}

criterion_group!(benches, struct_logger_benches);
criterion_main!(benches);
