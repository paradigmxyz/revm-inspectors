//! Isolates call-frame construction from execution and trace recording.

use alloy_primitives::{Address, Log};
use alloy_rpc_types_trace::geth::{erc7562::Erc7562Config, CallConfig};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use revm::{database_interface::EmptyDB, interpreter::InstructionResult};
use revm_inspectors::tracing::{
    types::{CallLog, CallTrace, CallTraceNode},
    GethTraceBuilder,
};
use std::hint::black_box;

fn nested_calls(count: usize) -> Vec<CallTraceNode> {
    (0..count)
        .map(|idx| CallTraceNode {
            idx,
            parent: idx.checked_sub(1),
            children: if idx + 1 < count { vec![idx + 1] } else { vec![] },
            trace: CallTrace {
                depth: idx,
                success: true,
                status: Some(InstructionResult::Return),
                ..Default::default()
            },
            logs: vec![CallLog::from(Log::new_unchecked(
                Address::ZERO,
                vec![],
                Default::default(),
            ))],
            ..Default::default()
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("geth_log_ancestry");
    for count in [8, 64, 256, 1024] {
        let builder = GethTraceBuilder::new(nested_calls(count));
        for with_log in [false, true] {
            group.bench_with_input(
                BenchmarkId::new(format!("call_with_log_{with_log}"), count),
                &builder,
                |b, builder| {
                    b.iter(|| {
                        black_box(builder).geth_call_traces(
                            CallConfig { with_log: Some(with_log), ..Default::default() },
                            0,
                        )
                    });
                },
            );
        }
        group.bench_with_input(
            BenchmarkId::new("erc7562_with_log", count),
            &builder,
            |b, builder| {
                b.iter(|| {
                    black_box(builder).geth_erc7562_traces(
                        Erc7562Config { with_log: Some(true), ..Default::default() },
                        0,
                        EmptyDB::default(),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
