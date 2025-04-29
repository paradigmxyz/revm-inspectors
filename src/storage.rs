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

/// An Inspector that tracks warm and cold storage slot accesses.
#[derive(Debug, Default)]
pub struct StorageInspector {
    /// Tracks storage slots and their access statistics per address
    accessed_slots: HashMap<Address, HashMap<B256, u64>>,
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
            .filter(|&&count| count == 1)
            .count() as u64
    }

    /// Returns the number of warm SLOAD operations  
    pub fn warm_loads(&self) -> u64 {
        self.accessed_slots
            .values()
            .flat_map(|slots| slots.values())
            .map(|&count| count.saturating_sub(1))
            .sum()
    }

    /// Consumes the inspector and returns the map
    pub fn into_inner(self) -> HashMap<Address, HashMap<B256, u64>> {
        self.accessed_slots
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

                let slot_access_count =
                    self.accessed_slots.entry(address).or_default().entry(slot).or_default();

                *slot_access_count += 1;
            }
        }
    }
}
