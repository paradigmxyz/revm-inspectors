use alloy_primitives::{map::HashMap, Address, B256};
use revm::{
    bytecode::opcode,
    context::ContextTr,
    inspector::JournalExt,
    interpreter::{
        interpreter_types::{InputsTr, Jumps},
        Interpreter,
    },
    Inspector,
};

/// Tracks storage slot access statistics
#[derive(Debug, Default)]
struct SlotStats {
    /// Number of times this slot was accessed when cold
    cold_loads: u64,
    /// Number of times this slot was accessed when warm
    warm_loads: u64,
}

/// An Inspector that tracks warm and cold storage slot accesses.
#[derive(Debug, Default)]
pub struct StorageInspector {
    /// Tracks storage slots and their access statistics per address
    accessed_slots: HashMap<Address, HashMap<B256, SlotStats>>,
}

impl StorageInspector {
    /// Creates a new storage inspector
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of cold SLOAD operations
    pub fn cold_loads(&self) -> u64 {
        self.accessed_slots
            .values()
            .flat_map(|slots| slots.values())
            .map(|stats| stats.cold_loads)
            .sum()
    }

    /// Returns the number of warm SLOAD operations  
    pub fn warm_loads(&self) -> u64 {
        self.accessed_slots
            .values()
            .flat_map(|slots| slots.values())
            .map(|stats| stats.warm_loads)
            .sum()
    }
}

impl<CTX> Inspector<CTX> for StorageInspector
where
    CTX: ContextTr<Journal: JournalExt>,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        if interp.bytecode.opcode() == opcode::SLOAD {
            if let Ok(slot) = interp.stack.peek(0) {
                let address = interp.input.target_address();
                let slot = B256::from(slot.to_be_bytes());

                let stats =
                    self.accessed_slots.entry(address).or_default().entry(slot).or_default();

                // If this is the first time this slot is accessed, it's a cold load
                if stats.cold_loads == 0 {
                    stats.cold_loads += 1;
                } else {
                    // If this slot has been accessed before, it's a warm load
                    stats.warm_loads += 1;
                }
            }
        }
    }
}
