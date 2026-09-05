//! Geth opcode-frame construction, excluding execution and recording.

use alloy_primitives::{Bytes, U256};
use alloy_rpc_types_trace::geth::GethDefaultTracingOptions;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use revm::{
    bytecode::opcode::{self, OpCode},
    interpreter::InstructionResult,
};
use revm_inspectors::tracing::{
    types::{CallTrace, CallTraceNode, CallTraceStep, StorageChange, StorageChangeReason},
    GethTraceBuilder,
};
use std::hint::black_box;

fn step(pc: usize, op: u8) -> CallTraceStep {
    CallTraceStep {
        pc,
        op: OpCode::new_or_unknown(op),
        stack: None,
        push_stack: None,
        memory: None,
        returndata: Bytes::new(),
        gas_remaining: 1_000_000,
        gas_refund_counter: 0,
        gas_used: 0,
        gas_cost: 3,
        state_gas_cost: None,
        state_gas_reservoir: None,
        state_gas_spent: 0,
        storage_change: None,
        status: None,
        immediate_bytes: None,
        decoded: None,
    }
}

fn node(idx: usize, parent: Option<usize>, depth: usize, steps: usize) -> CallTraceNode {
    CallTraceNode {
        idx,
        parent,
        trace: CallTrace {
            depth,
            success: true,
            status: Some(InstructionResult::Stop),
            steps: (0..steps)
                .map(|pc| step(pc, if pc % 2 == 0 { opcode::PUSH0 } else { opcode::POP }))
                .collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn bench(c: &mut Criterion) {
    let flat = vec![node(0, None, 0, 20_000)];
    let mut mixed = flat.clone();
    for step in mixed[0].trace.steps.iter_mut().step_by(16) {
        step.op = OpCode::new_or_unknown(opcode::SLOAD);
        step.storage_change = Some(Box::new(StorageChange {
            key: U256::ZERO,
            value: U256::from(1),
            had_value: None,
            reason: StorageChangeReason::SLOAD,
        }));
    }
    let mut wide = vec![node(0, None, 0, 4_000)];
    for idx in 1..=2_000 {
        wide[0].children.push(idx);
        wide[0].trace.steps[(idx - 1) * 2].op = OpCode::new_or_unknown(opcode::CALL);
        wide.push(node(idx, Some(0), 1, 20));
    }
    let mut deep: Vec<_> =
        (0usize..64).map(|idx| node(idx, idx.checked_sub(1), idx, 128)).collect();
    for (idx, node) in deep.iter_mut().enumerate().take(63) {
        node.children.push(idx + 1);
        node.trace.steps[64].op = OpCode::new_or_unknown(opcode::CALL);
    }
    let mut group = c.benchmark_group("geth_steps");
    for (name, nodes) in [
        ("flat_20000", flat),
        ("mixed_sload_20000", mixed),
        ("wide_2000x20", wide),
        ("deep_64x128", deep),
    ] {
        let builder = GethTraceBuilder::new(nodes);
        for disable_storage in [false, true] {
            group.bench_with_input(
                BenchmarkId::new(name, format!("disable_storage_{disable_storage}")),
                &builder,
                |b, builder| {
                    b.iter(|| {
                        black_box(builder).geth_traces(
                            0,
                            Bytes::new(),
                            GethDefaultTracingOptions {
                                disable_storage: Some(disable_storage),
                                disable_stack: Some(true),
                                ..Default::default()
                            },
                        )
                    })
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
