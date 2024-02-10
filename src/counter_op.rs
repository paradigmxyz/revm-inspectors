use revm::{
    interpreter::{opcode::OpCode, Interpreter},
    Database, EvmContext, Inspector,
};
use std::collections::HashMap;

/// An Inspector that counts opcodes and measures gas usage per opcode.
#[derive(Default, Debug)]
pub struct OpcodeCounterInspector {
    /// Map of opcode counts per transaction.
    opcode_counts: HashMap<OpCode, u64>,
    /// Map of total gas used per opcode.
    opcode_gas: HashMap<OpCode, u64>,
}

impl OpcodeCounterInspector {
    /// Creates a new instance of the inspector.
    pub fn new() -> Self {
        OpcodeCounterInspector { opcode_counts: HashMap::new(), opcode_gas: HashMap::new() }
    }

    /// Returns the opcode counts collected during transaction execution.
    pub fn opcode_counts(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_counts
    }

    /// Returns the opcode gas usage collected during transaction execution.
    pub fn opcode_gas(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_gas
    }
}

impl<DB> Inspector<DB> for OpcodeCounterInspector
where
    DB: Database,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let opcode_val = interp.current_opcode();

        if let Some(opcode) = OpCode::new(opcode_val) {
            *self.opcode_counts.entry(opcode).or_insert(0) += 1;

            
            // this should be upgraded to new release
            // when new ethereum version going to be updated in 2024/03
            let gas_info = revm::interpreter::instructions::opcode::spec_opcode_gas(
                revm::primitives::specification::SpecId::SHANGHAI,
            )[opcode_val as usize];
            let opcode_gas = gas_info.get_gas() as u64;

            // Increment gas usage for the opcode
            *self.opcode_gas.entry(opcode).or_insert(0) += opcode_gas;
        }
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

        let opcodes = [
            OpCode::new(opcode::PUSH1).unwrap(),
            OpCode::new(opcode::PUSH1).unwrap(),
            OpCode::new(opcode::SSTORE).unwrap(),
            OpCode::new(opcode::SSTORE).unwrap(),
            OpCode::new(opcode::SSTORE).unwrap(),
            OpCode::new(opcode::ADD).unwrap(),
            OpCode::new(opcode::MUL).unwrap(),
            OpCode::new(opcode::SUB).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
            OpCode::new(opcode::LOG3).unwrap(),
        ];

        for &opcode in &opcodes {
            interpreter.instruction_pointer = &opcode.get();
            opcode_counter.step(&mut interpreter, &mut EvmContext::new(db.clone()));
        }

        let opcode_counts = opcode_counter.opcode_counts();
        println!("Opcode Counts: {:?}", opcode_counts);

        let opcode_gas = opcode_counter.opcode_gas();
        println!("Opcode Gas Usage: {:?}", opcode_gas);
    }
}
