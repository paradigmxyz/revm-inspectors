use alloy_primitives::{Address, U256};
use core::hash::{Hash, Hasher};
use revm::{
    interpreter::{opcode::OpCode, Interpreter},
    Database, EvmContext, Inspector,
};

// This is the maximum number of edges that can be tracked. There is a tradeoff between performance
// and precision (less collisions).
const MAX_EDGE_COUNT: usize = 65536;

/// An `Inspector` that tracks [edge coverage](https://clang.llvm.org/docs/SanitizerCoverage.html#edge-coverage).
#[derive(Clone, Debug)]
pub struct EdgeCovInspector {
    /// Map of hitcounts that can be diffed against to determine if new coverage was reached.
    hitcount: Vec<NeverZeroHitCount>,
}

impl EdgeCovInspector {
    /// Create a new `EdgeCovInspector` with `MAX_EDGE_COUNT` size.
    pub fn new() -> Self {
        Self { hitcount: vec![NeverZeroHitCount(0); MAX_EDGE_COUNT] }
    }

    /// Reset the hitcount to zero.
    pub fn reset(&mut self) {
        self.hitcount.fill(NeverZeroHitCount(0));
    }

    /// Get the hitcount as a byte vector.
    pub fn get_hitcount(&self) -> Vec<u8> {
        self.hitcount.iter().map(|x| x.0).collect()
    }
}

impl Default for EdgeCovInspector {
    fn default() -> Self {
        Self::new()
    }
}

/// The edge hash is a combination of the address, the current program counter, and the jump
/// destination. The hash is used to index into the hitcount array, so it must be modulo the maximum
/// edge count.
fn edge_hash(address: Address, pc: usize, jump_dest: U256) -> u32 {
    // TODO faster hash https://h0mbre.github.io/Lucid_Snapshots_Coverage/
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    address.hash(&mut hasher);
    pc.hash(&mut hasher);
    jump_dest.hash(&mut hasher);
    (hasher.finish() as usize % MAX_EDGE_COUNT) as u32
}

impl<DB> Inspector<DB> for EdgeCovInspector
where
    DB: Database,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let address = interp.contract.target_address; // TODO track context for delegatecall?
        let current_pc = interp.program_counter();
        let opcode_value = interp.current_opcode();
        if let Some(op) = OpCode::new(opcode_value) {
            match op {
                OpCode::JUMP => {
                    // unconditional jump
                    if let Ok(jump_dest) = interp.stack().peek(0) {
                        let edge_id = edge_hash(address, current_pc, jump_dest);
                        self.hitcount[edge_id as usize] += NeverZeroHitCount(1);
                    }
                }
                OpCode::JUMPI => {
                    if let Ok(stack_value) = interp.stack().peek(0) {
                        let jump_dest = if stack_value == U256::from(1) {
                            // branch taken
                            interp.stack().peek(1).unwrap()
                        } else {
                            // fall through
                            U256::from(current_pc + 1)
                        };
                        let edge_id = edge_hash(address, current_pc, jump_dest);
                        self.hitcount[edge_id as usize] += NeverZeroHitCount(1);
                    }
                }
                _ => {
                    // no-op
                }
            }
        }
    }
}

/// A hit count that never goes to zero e.g. the back edge of a loop that iterates 256 times will be
/// one instead of zero.
// see https://github.com/AFLplusplus/AFLplusplus/blob/5777ceaf23f48ae4ceae60e4f3a79263802633c6/instrumentation/afl-llvm-pass.so.cc#L810-L829
#[derive(Clone, Copy, Debug, Default)]
pub struct NeverZeroHitCount(pub u8);

impl std::ops::Add for NeverZeroHitCount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self(self.0.checked_add(rhs.0).unwrap_or(1))
    }
}

impl std::ops::AddAssign for NeverZeroHitCount {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonzero_hitcount() {
        let mut hitcount = NeverZeroHitCount(255);
        hitcount += NeverZeroHitCount(1);
        assert_eq!(hitcount.0, 1);
    }
}
