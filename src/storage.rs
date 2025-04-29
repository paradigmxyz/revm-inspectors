use alloc::collections::BTreeSet;
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
    /// Tracks which storage slots have been accessed
    accessed_slots: HashMap<Address, BTreeSet<B256>>,
    /// Counter for cold SLOADs
    cold_loads: u64,
    /// Counter for warm SLOADs  
    warm_loads: u64,
}

impl StorageInspector {
    /// Creates a new storage inspector
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of cold SLOAD operations
    pub fn cold_loads(&self) -> u64 {
        self.cold_loads
    }

    /// Returns the number of warm SLOAD operations  
    pub fn warm_loads(&self) -> u64 {
        self.warm_loads
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

                // Check if this slot was previously accessed
                if let Some(slots) = self.accessed_slots.get(&address) {
                    if slots.contains(&slot) {
                        self.warm_loads += 1;
                        return;
                    }
                }

                // First time access - mark as cold
                self.cold_loads += 1;
                self.accessed_slots.entry(address).or_default().insert(slot);
            }
        }
    }
}
