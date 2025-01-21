use alloy_primitives::{map::DefaultHashBuilder, Address, U256};
use core::hash::{BuildHasher, Hash, Hasher};
use revm::{
    interpreter::{
        opcode::{self},
        Interpreter,
    },
    Database, EvmContext, Inspector,
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

    /// Get the hitcount as a byte vector.
    pub fn get_hitcount(&self) -> &[u8] {
        self.hitcount.as_slice()
    }

    /// The edge hash is a combination of the address, the current program counter, and the jump
    /// destination. The hash is used to index into the hitcount array, so it must be modulo the
    /// maximum edge count.
    fn edge_hash(&self, address: Address, pc: usize, jump_dest: U256) -> u32 {
        let mut hasher = self.hash_builder.build_hasher();
        address.hash(&mut hasher);
        pc.hash(&mut hasher);
        jump_dest.hash(&mut hasher);
        (hasher.finish() as usize % MAX_EDGE_COUNT) as u32
    }
}

impl Default for EdgeCovInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl<DB> Inspector<DB> for EdgeCovInspector
where
    DB: Database,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let address = interp.contract.target_address; // TODO track context for delegatecall?
        let current_pc = interp.program_counter();

        match interp.current_opcode() {
            opcode::JUMP => {
                // unconditional jump
                if let Ok(jump_dest) = interp.stack().peek(0) {
                    let edge_id = self.edge_hash(address, current_pc, jump_dest) as usize;
                    self.hitcount[edge_id] = self.hitcount[edge_id].checked_add(1).unwrap_or(1);
                }
            }
            opcode::JUMPI => {
                if let Ok(stack_value) = interp.stack().peek(0) {
                    if let Ok(jump_dest) = if stack_value != U256::from(0) {
                        // branch taken
                        interp.stack().peek(1)
                    } else {
                        // fall through
                        Ok(U256::from(current_pc + 1))
                    } {
                        let edge_id = self.edge_hash(address, current_pc, jump_dest) as usize;
                        self.hitcount[edge_id] = self.hitcount[edge_id].checked_add(1).unwrap_or(1);
                    }
                }
            }
            _ => {
                // no-op
            }
        }
    }
}
