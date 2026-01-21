use alloc::{vec, vec::Vec};
use alloy_primitives::{map::DefaultHashBuilder, Address, U256};
use core::{
    fmt,
    hash::{BuildHasher, Hash, Hasher},
};
use revm::{
    bytecode::opcode::{self},
    interpreter::{
        interpreter_types::{InputsTr, Jumps},
        Interpreter,
    },
    Inspector,
};

// This is the maximum number of edges that can be tracked. There is a tradeoff between performance
// and precision (less collisions).
const MAX_EDGE_COUNT: usize = 65536;

// Maximum number of comparison operand pairs to track (for CmpLog-style feedback).
const MAX_CMP_LOG_ENTRIES: usize = 1024;

/// A comparison operand pair captured during execution.
/// Used for CmpLog-style guided fuzzing to help solve constraints.
#[derive(Clone, Copy, Debug, Default)]
pub struct CmpOperands {
    /// First operand of the comparison
    pub op1: U256,
    /// Second operand of the comparison
    pub op2: U256,
    /// Program counter where the comparison occurred
    pub pc: usize,
}

/// An `Inspector` that tracks [edge coverage](https://clang.llvm.org/docs/SanitizerCoverage.html#edge-coverage).
/// Covered edges will not wrap to zero e.g. a loop edge hit more than 255 will still be retained.
///
/// Also tracks comparison operands (CmpLog-style) to enable constraint solving during fuzzing.
// see https://github.com/AFLplusplus/AFLplusplus/blob/5777ceaf23f48ae4ceae60e4f3a79263802633c6/instrumentation/afl-llvm-pass.so.cc#L810-L829
#[derive(Clone)]
pub struct EdgeCovInspector {
    /// Map of hitcounts that can be diffed against to determine if new coverage was reached.
    hitcount: Vec<u8>,
    hash_builder: DefaultHashBuilder,
    /// Comparison operand log for CmpLog-style guided fuzzing.
    /// Stores operands from EQ, LT, GT, SLT, SGT operations.
    cmp_log: Vec<CmpOperands>,
}

impl fmt::Debug for EdgeCovInspector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EdgeCovInspector").finish_non_exhaustive()
    }
}

impl EdgeCovInspector {
    /// Create a new `EdgeCovInspector` with `MAX_EDGE_COUNT` size.
    pub fn new() -> Self {
        Self {
            hitcount: vec![0; MAX_EDGE_COUNT],
            hash_builder: DefaultHashBuilder::default(),
            cmp_log: Vec::with_capacity(MAX_CMP_LOG_ENTRIES),
        }
    }

    /// Reset the hitcount to zero and clear comparison log.
    pub fn reset(&mut self) {
        self.hitcount.fill(0);
        self.cmp_log.clear();
    }

    /// Get an immutable reference to the hitcount.
    pub fn get_hitcount(&self) -> &[u8] {
        self.hitcount.as_slice()
    }

    /// Get an immutable reference to the comparison operand log.
    pub fn get_cmp_log(&self) -> &[CmpOperands] {
        self.cmp_log.as_slice()
    }

    /// Consume the inspector and take ownership of the hitcount.
    pub fn into_hitcount(self) -> Vec<u8> {
        self.hitcount
    }

    /// Consume the inspector and return both hitcount and cmp_log.
    pub fn into_parts(self) -> (Vec<u8>, Vec<CmpOperands>) {
        (self.hitcount, self.cmp_log)
    }

    /// Mark the edge, H(address, pc, jump_dest), as hit.
    fn store_hit(&mut self, address: Address, pc: usize, jump_dest: U256) {
        let mut hasher = self.hash_builder.build_hasher();
        address.hash(&mut hasher);
        pc.hash(&mut hasher);
        jump_dest.hash(&mut hasher);
        // The hash is used to index into the hitcount array,
        // so it must be modulo the maximum edge count.
        let edge_id = (hasher.finish() % MAX_EDGE_COUNT as u64) as usize;
        self.hitcount[edge_id] = self.hitcount[edge_id].checked_add(1).unwrap_or(1);
    }

    /// Store comparison operands for CmpLog-style guided fuzzing.
    fn store_cmp(&mut self, pc: usize, op1: U256, op2: U256) {
        if self.cmp_log.len() < MAX_CMP_LOG_ENTRIES {
            self.cmp_log.push(CmpOperands { op1, op2, pc });
        }
    }

    #[cold]
    fn do_step(&mut self, interp: &mut Interpreter) {
        let address = interp.input.target_address(); // TODO track context for delegatecall?
        let current_pc = interp.bytecode.pc();

        match interp.bytecode.opcode() {
            opcode::JUMP => {
                // unconditional jump
                if let Ok(jump_dest) = interp.stack.peek(0) {
                    self.store_hit(address, current_pc, jump_dest);
                }
            }
            opcode::JUMPI => {
                if let Ok(stack_value) = interp.stack.peek(1) {
                    let jump_dest = if !stack_value.is_zero() {
                        // branch taken
                        interp.stack.peek(0)
                    } else {
                        // fall through
                        Ok(U256::from(current_pc + 1))
                    };

                    if let Ok(jump_dest) = jump_dest {
                        self.store_hit(address, current_pc, jump_dest);
                    }
                }
            }
            _ => {
                // no-op
            }
        }
    }

    /// Handle comparison opcodes to log operands for CmpLog-style guidance.
    #[cold]
    fn do_cmp_step(&mut self, interp: &mut Interpreter) {
        let current_pc = interp.bytecode.pc();

        match interp.bytecode.opcode() {
            opcode::EQ | opcode::LT | opcode::GT | opcode::SLT | opcode::SGT => {
                // These opcodes compare stack[0] and stack[1]
                if let (Ok(op1), Ok(op2)) = (interp.stack.peek(0), interp.stack.peek(1)) {
                    self.store_cmp(current_pc, op1, op2);
                }
            }
            opcode::ISZERO => {
                // ISZERO compares stack[0] with 0
                if let Ok(op1) = interp.stack.peek(0) {
                    self.store_cmp(current_pc, op1, U256::ZERO);
                }
            }
            _ => {
                // no-op
            }
        }
    }
}

impl Default for EdgeCovInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl<CTX> Inspector<CTX> for EdgeCovInspector {
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        let op = interp.bytecode.opcode();
        if matches!(op, opcode::JUMP | opcode::JUMPI) {
            self.do_step(interp);
        }
        // Track comparison operands for CmpLog-style guidance
        if matches!(
            op,
            opcode::EQ | opcode::LT | opcode::GT | opcode::SLT | opcode::SGT | opcode::ISZERO
        ) {
            self.do_cmp_step(interp);
        }
    }
}
