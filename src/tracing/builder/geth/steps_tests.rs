use super::*;
use crate::tracing::types::{CallTrace, CallTraceStep, StorageChange, StorageChangeReason};
use revm::interpreter::InstructionResult;

fn test_step(pc: usize, op: u8) -> CallTraceStep {
    CallTraceStep {
        pc,
        op: opcode::OpCode::new_or_unknown(op),
        stack: Some(vec![U256::from(pc)].into_boxed_slice()),
        push_stack: None,
        memory: None,
        returndata: Bytes::from(vec![pc as u8]),
        gas_remaining: 1000 - pc as u64,
        gas_refund_counter: 0,
        gas_used: pc as u64,
        gas_cost: 1,
        state_gas_cost: None,
        state_gas_reservoir: None,
        state_gas_spent: 0,
        storage_change: (op == opcode::SSTORE).then(|| {
            alloc::boxed::Box::new(StorageChange {
                key: U256::ZERO,
                value: U256::from(pc),
                had_value: None,
                reason: StorageChangeReason::SSTORE,
            })
        }),
        status: (op == opcode::REVERT).then_some(InstructionResult::Revert),
        immediate_bytes: None,
        decoded: None,
    }
}

#[test]
fn opcode_trace_resumes_parents_after_nested_and_empty_calls() {
    // Cover all call-like opcodes, a precompile with no steps, an empty creation,
    // a reverting grandchild, and a final call opcode that failed before recording a child.
    type Fixture = (Option<usize>, usize, &'static [usize], &'static [u8]);
    let fixtures: &[Fixture] = &[
        (
            None,
            0,
            &[1, 2, 4, 5, 6],
            &[
                opcode::SSTORE,
                opcode::CALL,
                opcode::CALLCODE,
                opcode::STATICCALL,
                opcode::CREATE,
                opcode::CREATE2,
                opcode::CALL,
            ],
        ),
        (Some(0), 1, &[], &[]),
        (Some(0), 1, &[3], &[opcode::SSTORE, opcode::DELEGATECALL, opcode::SLOAD, opcode::STOP]),
        (Some(2), 2, &[], &[opcode::SSTORE, opcode::REVERT]),
        (Some(0), 1, &[], &[opcode::RETURN]),
        (Some(0), 1, &[], &[]),
        (Some(0), 1, &[], &[opcode::STOP]),
    ];
    let mut nodes: Vec<_> = fixtures
        .iter()
        .enumerate()
        .map(|(idx, &(parent, depth, children, ops))| {
            CallTraceNode {
                idx,
                parent,
                children: children.to_vec(),
                trace: CallTrace {
                    depth,
                    maybe_precompile: Some(idx == 1),
                    address: Address::with_last_byte(idx as u8),
                    // The grandchild executes in its parent's storage context.
                    caller: Address::with_last_byte(2),
                    kind: if idx == 3 { CallKind::DelegateCall } else { CallKind::Call },
                    steps: ops
                        .iter()
                        .enumerate()
                        .map(|(pc, &op)| test_step(idx * 10 + pc, op))
                        .collect(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .collect();
    nodes[0].trace.steps.last_mut().unwrap().status = Some(InstructionResult::OutOfGas);
    let builder = GethTraceBuilder::new(nodes);
    let expected = [
        (0, 1),
        (1, 1),
        (2, 1),
        (20, 2),
        (21, 2),
        (30, 3),
        (31, 3),
        (22, 2),
        (23, 2),
        (3, 1),
        (40, 2),
        (4, 1),
        (5, 1),
        (60, 2),
        (6, 1),
    ];
    for disable_storage in [false, true] {
        let opts = GethDefaultTracingOptions {
            disable_storage: Some(disable_storage),
            enable_return_data: Some(true),
            ..Default::default()
        };
        let frame = builder.geth_traces(123, Bytes::new(), opts);
        assert_eq!(frame.gas, 123);
        assert_eq!(
            frame.struct_logs.iter().map(|log| (log.pc, log.depth)).collect::<Vec<_>>(),
            expected
        );
        for log in &frame.struct_logs {
            assert_eq!(log.stack, Some(vec![U256::from(log.pc)]));
            assert_eq!(log.return_data, Some(Bytes::from(vec![log.pc as u8])));
            assert_eq!(log.gas, 1000 - log.pc);
            let value = match log.pc {
                0 => Some(0),
                20 => Some(20),
                30 | 22 => Some(30),
                _ => None,
            };
            assert_eq!(
                log.storage,
                value
                    .filter(|_| !disable_storage)
                    .map(|value| { BTreeMap::from([(B256::ZERO, U256::from(value).into())]) })
            );
        }
    }
    assert!(GethTraceBuilder::new(vec![])
        .geth_traces(0, Bytes::new(), Default::default())
        .struct_logs
        .is_empty());
    assert!(GethTraceBuilder::new(vec![CallTraceNode::default()])
        .geth_traces(0, Bytes::new(), Default::default())
        .struct_logs
        .is_empty());
}

#[test]
fn storage_snapshots_keep_empty_reads_and_custom_changes() {
    let mut custom_change = test_step(2, opcode::SSTORE);
    // Public trace nodes can carry a storage change on a different opcode.
    custom_change.op = opcode::OpCode::new_or_unknown(opcode::ADD);
    let builder = GethTraceBuilder::new(vec![CallTraceNode {
        trace: CallTrace {
            steps: vec![
                test_step(0, opcode::PUSH0),
                test_step(1, opcode::SLOAD),
                custom_change,
                test_step(3, opcode::SLOAD),
            ],
            ..Default::default()
        },
        ..Default::default()
    }]);
    for disable_storage in [false, true] {
        let frame = builder.geth_traces(
            0,
            Bytes::new(),
            GethDefaultTracingOptions {
                disable_storage: Some(disable_storage),
                ..Default::default()
            },
        );
        assert_eq!(frame.struct_logs.len(), 4);
        assert!(frame.struct_logs[0].storage.is_none());
        assert_eq!(frame.struct_logs[1].storage, (!disable_storage).then(BTreeMap::new));
        assert!(frame.struct_logs[2].storage.is_none());
        assert_eq!(
            frame.struct_logs[3].storage,
            (!disable_storage).then(|| { BTreeMap::from([(B256::ZERO, U256::from(2).into())]) })
        );
    }
}
