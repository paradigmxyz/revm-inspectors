use revm::interpreter::{opcode::OpCode, Interpreter};

use std::collections::HashMap;

/// An Inspector that counts all opcodes executed during a transaction.
#[derive(Default, Debug)]
pub struct OpcodeCounterInspector {
    /// Map of opcode counts per transaction.
    pub opcode_counts: HashMap<OpCode, u64>,
}

impl OpcodeCounterInspector {
    /// Creates a new instance of the inspector.
    pub fn new() -> Self {
        OpcodeCounterInspector { opcode_counts: HashMap::new() }
    }

    /// Returns the opcode counts collected during transaction execution.
    pub fn opcode_counts(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_counts
    }
}

impl<DB> revm::Inspector<DB> for OpcodeCounterInspector
where
    DB: revm::Database,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut revm::EvmContext<DB>) {
        let opcode_value = interp.current_opcode();
        let opcode = OpCode::new(opcode_value)
            .unwrap_or_else(|| unsafe { OpCode::new_unchecked(opcode_value) });

        let count = self.opcode_counts.entry(opcode).or_insert(0);
        *count += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use revm::{
        db::{CacheDB, EmptyDB},
        interpreter::{opcode, Contract, Interpreter},
        EvmContext, Inspector,
    };

    #[test]
    fn test_opcode_counter_inspector() {
        let mut opcode_counter = OpcodeCounterInspector::new();

        let contract = Box::new(Contract::default());
        let mut interpreter = Interpreter::new(contract, 10000, false);
        let db = CacheDB::new(EmptyDB::default());

        let opcode_push1 = OpCode::new(opcode::PUSH1).unwrap();
        interpreter.instruction_pointer = &opcode_push1.get();
        opcode_counter.step(&mut interpreter, &mut EvmContext::new(db.clone()));

        let opcode_sstore = OpCode::new(opcode::SSTORE).unwrap();
        interpreter.instruction_pointer = &opcode_sstore.get();
        opcode_counter.step(&mut interpreter, &mut EvmContext::new(db));

        let opcode_counts = opcode_counter.opcode_counts();
        println!("{:?}", opcode_counts);
    }
}
