use alloc::{vec, vec::Vec};
use alloy_primitives::{
    map::{Entry, HashMap},
    Address, U256,
};
use core::fmt;
use revm::{
    bytecode::opcode::{self},
    interpreter::{
        interpreter_types::{InputsTr, Jumps},
        Interpreter,
    },
    Inspector,
};

/// Default capacity for the hitcount buffer. Pre-allocated to avoid
/// repeated growth; only `[0..next_id]` entries are meaningful.
const DEFAULT_MAP_CAPACITY: usize = 65536;

/// An `Inspector` that tracks
/// [edge coverage](https://clang.llvm.org/docs/SanitizerCoverage.html#edge-coverage)
/// using collision-free dense edge IDs.
///
/// Each unique `(address, pc, jump_dest)` edge is assigned a
/// monotonically-increasing ID the first time it is observed. The ID
/// indexes into a pre-allocated hitcount buffer, so two distinct edges
/// never share the same counter.
///
/// The hitcount buffer is fixed at construction time; if more unique edges
/// are discovered than the buffer can hold the extras are silently
/// dropped. Use [`EdgeCovInspector::with_capacity`] to size the buffer
/// for your workload.
// see https://github.com/AFLplusplus/AFLplusplus/blob/5777ceaf23f48ae4ceae60e4f3a79263802633c6/instrumentation/afl-llvm-pass.so.cc#L810-L829
#[derive(Clone)]
pub struct EdgeCovInspector {
    /// Pre-allocated hitcount buffer indexed by dense edge ID.
    hitcount: Vec<u8>,
    /// `(address, pc, jump_dest)` → dense ID.
    edge_ids: HashMap<(Address, usize, U256), usize>,
    /// Next ID to assign (= number of unique edges discovered so far).
    next_id: usize,
}

impl fmt::Debug for EdgeCovInspector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EdgeCovInspector")
            .field("capacity", &self.hitcount.len())
            .field("edges", &self.next_id)
            .finish()
    }
}

impl EdgeCovInspector {
    /// Create a new `EdgeCovInspector` with the default capacity (65536).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAP_CAPACITY)
    }

    /// Create a new `EdgeCovInspector` with the given hitcount buffer
    /// capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { hitcount: vec![0; capacity], edge_ids: HashMap::default(), next_id: 0 }
    }

    /// Reset the hitcount to zero without discarding the edge ID
    /// mapping. Call this between fuzz iterations when you want to
    /// keep the same ID assignments but start fresh hit counters.
    pub fn reset(&mut self) {
        let used = self.next_id.min(self.hitcount.len());
        self.hitcount[..used].fill(0);
    }

    /// Get an immutable reference to the *used* portion of the
    /// hitcount buffer (`[0..edge_count()]`).
    pub fn get_hitcount(&self) -> &[u8] {
        let used = self.next_id.min(self.hitcount.len());
        &self.hitcount[..used]
    }

    /// Get a mutable reference to the full hitcount buffer.
    ///
    /// External writers (e.g. sancov callbacks) that also record
    /// coverage into this buffer should use a dedicated range that
    /// does not overlap with EVM-assigned IDs. A safe approach is to
    /// write into `[edge_count()..]` *after* EVM execution is
    /// complete so that no new EVM IDs will be allocated.
    pub fn hitcount_mut(&mut self) -> &mut [u8] {
        &mut self.hitcount
    }

    /// Number of unique edges discovered so far.
    pub fn edge_count(&self) -> usize {
        self.next_id
    }

    /// Consume the inspector and return `(hitcount_buffer, used_size)`.
    ///
    /// Only `hitcount[0..used_size]` contains meaningful data; the rest
    /// is unused pre-allocated space.
    pub fn into_hitcount_with_size(self) -> (Vec<u8>, usize) {
        let used = self.next_id.min(self.hitcount.len());
        (self.hitcount, used)
    }

    /// Consume the inspector and take ownership of the hitcount.
    ///
    /// For backward compatibility this returns the full buffer. Prefer
    /// [`into_hitcount_with_size`](Self::into_hitcount_with_size) to
    /// also obtain the number of valid entries.
    pub fn into_hitcount(self) -> Vec<u8> {
        self.hitcount
    }

    /// Mark the edge `(address, pc, jump_dest)` as hit, assigning a
    /// new dense ID on first encounter.
    fn store_hit(&mut self, address: Address, pc: usize, jump_dest: U256) {
        let id = match self.edge_ids.entry((address, pc, jump_dest)) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let id = self.next_id;
                if id >= self.hitcount.len() {
                    return; // buffer exhausted
                }
                self.next_id += 1;
                e.insert(id);
                id
            }
        };
        if let Some(slot) = self.hitcount.get_mut(id) {
            *slot = slot.saturating_add(1);
        }
    }

    #[cold]
    fn do_step(&mut self, interp: &mut Interpreter) {
        let address = interp.input.target_address();
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
}

impl Default for EdgeCovInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl<CTX> Inspector<CTX> for EdgeCovInspector {
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        if matches!(interp.bytecode.opcode(), opcode::JUMP | opcode::JUMPI) {
            self.do_step(interp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_free_ids() {
        let mut inspector = EdgeCovInspector::new();
        let addr = Address::ZERO;

        // Record three distinct edges.
        inspector.store_hit(addr, 0, U256::from(10));
        inspector.store_hit(addr, 0, U256::from(20));
        inspector.store_hit(addr, 1, U256::from(10));

        // Each should get its own ID → 3 unique non-zero slots.
        assert_eq!(inspector.edge_count(), 3);
        let hc = inspector.get_hitcount();
        assert_eq!(hc.len(), 3);
        assert!(hc.iter().all(|&x| x == 1));
    }

    #[test]
    fn same_edge_increments_same_slot() {
        let mut inspector = EdgeCovInspector::new();
        let addr = Address::ZERO;

        for _ in 0..5 {
            inspector.store_hit(addr, 42, U256::from(100));
        }

        assert_eq!(inspector.edge_count(), 1);
        assert_eq!(inspector.get_hitcount(), &[5]);
    }

    #[test]
    fn hitcount_saturates_at_255() {
        let mut inspector = EdgeCovInspector::new();
        let addr = Address::ZERO;

        for _ in 0..300 {
            inspector.store_hit(addr, 0, U256::from(1));
        }

        assert_eq!(inspector.edge_count(), 1);
        assert_eq!(inspector.get_hitcount(), &[255]);
    }

    #[test]
    fn capacity_exhaustion_drops_new_edges() {
        let mut inspector = EdgeCovInspector::with_capacity(2);
        let addr = Address::ZERO;

        inspector.store_hit(addr, 0, U256::from(1)); // id 0
        inspector.store_hit(addr, 0, U256::from(2)); // id 1
        inspector.store_hit(addr, 0, U256::from(3)); // dropped

        assert_eq!(inspector.edge_count(), 2);
        let hc = inspector.get_hitcount();
        assert_eq!(hc.len(), 2);
        assert_eq!(hc, &[1, 1]);
    }

    #[test]
    fn reset_clears_hitcounts_preserves_ids() {
        let mut inspector = EdgeCovInspector::new();
        let addr = Address::ZERO;

        inspector.store_hit(addr, 0, U256::from(1));
        inspector.store_hit(addr, 0, U256::from(2));
        assert_eq!(inspector.edge_count(), 2);
        assert!(inspector.get_hitcount().iter().all(|&x| x == 1));

        inspector.reset();
        assert_eq!(inspector.edge_count(), 2);
        assert!(inspector.get_hitcount().iter().all(|&x| x == 0));

        // Re-hitting the same edges should reuse their IDs.
        inspector.store_hit(addr, 0, U256::from(1));
        assert_eq!(inspector.edge_count(), 2);
        assert_eq!(inspector.get_hitcount(), &[1, 0]);
    }

    #[test]
    fn into_hitcount_with_size_returns_used() {
        let mut inspector = EdgeCovInspector::with_capacity(1024);
        let addr = Address::ZERO;

        inspector.store_hit(addr, 0, U256::from(1));
        inspector.store_hit(addr, 1, U256::from(2));

        let (buf, used) = inspector.into_hitcount_with_size();
        assert_eq!(used, 2);
        assert_eq!(buf.len(), 1024);
        assert_eq!(buf[0], 1);
        assert_eq!(buf[1], 1);
        assert!(buf[2..].iter().all(|&x| x == 0));
    }

    #[test]
    fn different_addresses_different_edges() {
        let mut inspector = EdgeCovInspector::new();
        let addr_a = Address::ZERO;
        let addr_b = Address::with_last_byte(1);

        // Same (pc, jump_dest) but different contract address →
        // distinct edges.
        inspector.store_hit(addr_a, 0, U256::from(10));
        inspector.store_hit(addr_b, 0, U256::from(10));

        assert_eq!(inspector.edge_count(), 2);
    }

    #[test]
    fn debug_format() {
        let inspector = EdgeCovInspector::with_capacity(128);
        let dbg = format!("{inspector:?}");
        assert!(dbg.contains("capacity: 128"));
        assert!(dbg.contains("edges: 0"));
    }
}
