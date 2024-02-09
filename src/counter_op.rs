use revm::{interpreter::Interpreter, Database, EvmContext, Inspector};
use std::collections::HashMap;

/// An Inspector that counts opcodes executed during a transaction.
#[derive(Default, Debug)]
pub struct OpcodeCounterInspector {
    /// Map of opcode counts per transaction.
    opcode_counts: HashMap<u8, u64>,
}

impl OpcodeCounterInspector {
    /// Creates a new instance of the inspector.
    pub fn new() -> Self {
        OpcodeCounterInspector { opcode_counts: HashMap::new() }
    }

    /// Returns the opcode counts collected during transaction execution.
    pub fn opcode_counts(&self) -> &HashMap<u8, u64> {
        &self.opcode_counts
    }
}

impl<DB> Inspector<DB> for OpcodeCounterInspector
where
    DB: Database,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let opcode_value = interp.current_opcode();

        let count = self.opcode_counts.entry(opcode_value).or_insert(0);
        *count += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use revm::{
        db::{CacheDB, EmptyDB},
        interpreter::{instructions::opcode, Contract, Interpreter},
    };

    #[test]
    fn test_opcode_counter_inspector() {
        
        let mut opcode_counter = OpcodeCounterInspector::new();

        let contract = Box::new(Contract::default());

        let mut interpreter = Interpreter::new(contract, 10000, false);

        let db = CacheDB::new(EmptyDB::default());
        interpreter.instruction_pointer = &opcode::PUSH1;
        opcode_counter.step(&mut interpreter, &mut EvmContext::new(db.clone()));

        interpreter.instruction_pointer = &opcode::SSTORE;
        opcode_counter.step(&mut interpreter, &mut EvmContext::new(db));

        let opcode_counts = opcode_counter.opcode_counts();
        println!("{:?}", opcode_counts);
    }
}
