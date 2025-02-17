use alloc::{vec, vec::Vec};
use alloy_primitives::{map::DefaultHashBuilder, Address, U256};
use core::hash::{BuildHasher, Hash, Hasher};
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

/// An `Inspector` that tracks [edge coverage](https://clang.llvm.org/docs/SanitizerCoverage.html#edge-coverage).
/// Covered edges will not wrap to zero e.g. a loop edge hit more than 255 will still be retained.
// see https://github.com/AFLplusplus/AFLplusplus/blob/5777ceaf23f48ae4ceae60e4f3a79263802633c6/instrumentation/afl-llvm-pass.so.cc#L810-L829
#[derive(Clone, Debug)]
pub struct EdgeCovInspector {
    /// Map of hitcounts that can be diffed against to determine if new coverage was reached.
    hitcount: Vec<u8>,
    hash_builder: DefaultHashBuilder,
}

impl EdgeCovInspector {
    /// Create a new `EdgeCovInspector` with `MAX_EDGE_COUNT` size.
    pub fn new() -> Self {
        Self { hitcount: vec![0; MAX_EDGE_COUNT], hash_builder: DefaultHashBuilder::default() }
    }

    /// Reset the hitcount to zero.
    pub fn reset(&mut self) {
        self.hitcount.fill(0);
    }

    /// Get an immutable reference to the hitcount.
    pub fn get_hitcount(&self) -> &[u8] {
        self.hitcount.as_slice()
    }

    /// Consume the inspector and take ownership of the hitcount.
    pub fn into_hitcount(self) -> Vec<u8> {
        self.hitcount
    }

    /// Mark the edge, H(address, pc, jump_dest), as hit.
    fn store_hit(&mut self, address: Address, pc: usize, jump_dest: U256) {
        let mut hasher = self.hash_builder.build_hasher();
        address.hash(&mut hasher);
        pc.hash(&mut hasher);
        jump_dest.hash(&mut hasher);
        // The hash is used to index into the hitcount array, so it must be modulo the maximum edge
        // count.
        let edge_id = (hasher.finish() % MAX_EDGE_COUNT as u64) as usize;
        self.hitcount[edge_id] = self.hitcount[edge_id].checked_add(1).unwrap_or(1);
    }
}

impl Default for EdgeCovInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl<CTX> Inspector<CTX> for EdgeCovInspector {
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
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
                if let Ok(stack_value) = interp.stack.peek(0) {
                    let jump_dest = if !stack_value.is_zero() {
                        // branch taken
                        interp.stack.peek(1)
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
}
