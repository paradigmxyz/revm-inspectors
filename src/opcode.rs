use revm::{
    inspectors::GasInspector,
    interpreter::{opcode::OpCode, Interpreter},
    Database, EvmContext, Inspector,
};
use std::collections::HashMap;
/// An Inspector that counts opcodes and measures gas usage per opcode.
#[derive(Clone, Debug, Default)]
pub struct OpcodeCounterInspector {
    /// Map of opcode counts per transaction.
    pub opcode_counts: HashMap<OpCode, u64>,
    /// Map of total gas used per opcode.
    pub opcode_gas: HashMap<OpCode, u64>,
    gas_inspector: GasInspector,
}

impl OpcodeCounterInspector {
    /// Creates a new instance of the inspector.
    pub fn new() -> Self {
        OpcodeCounterInspector {
            opcode_counts: HashMap::new(),
            opcode_gas: HashMap::new(),
            gas_inspector: GasInspector::default(),
        }
    }

    /// Returns the opcode counts collected during transaction execution.
    pub const fn opcode_counts(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_counts
    }

    /// Returns the opcode gas usage collected during transaction execution.
    pub const fn opcode_gas(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_gas
    }

    /// Returns an iterator over all opcodes with their count and gas usage.
    pub fn iter_opcodes(&self) -> impl Iterator<Item = (OpCode, (u64, u64))> + '_ {
        self.opcode_counts.iter().map(move |(&opcode, &count)| {
            let gas = self.opcode_gas.get(&opcode).copied().unwrap_or_default();
            (opcode, (count, gas))
        })
    }
}

impl<DB> Inspector<DB> for OpcodeCounterInspector
where
    DB: Database,
{
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        self.initialize_interp(interp, context);

        let opcode_value = interp.current_opcode();
        if let Some(opcode) = OpCode::new(opcode_value) {
            *self.opcode_counts.entry(opcode).or_insert(0) += 1;
        }

        self.step_end(interp, context);
        let gas_table = revm::interpreter::instructions::opcode::spec_opcode_gas(context.spec_id());
        let opcode_gas_info = gas_table[opcode_value as usize];

        let opcode_gas_cost = opcode_gas_info.get_gas() as u64;

        if let Some(opcode) = OpCode::new(opcode_value) {
            let gas_used = self.gas_inspector.last_gas_cost();
            *self.opcode_gas.entry(opcode).or_insert(0) += gas_used + opcode_gas_cost;
        }
    }

    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        self.gas_inspector.step_end(interp, context);
    }
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut revm::interpreter::CallInputs,
    ) -> Option<revm::interpreter::CallOutcome> {
        self.gas_inspector.call(context, inputs)
    }
    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &revm::interpreter::CallInputs,
        outcome: revm::interpreter::CallOutcome,
    ) -> revm::interpreter::CallOutcome {
        self.gas_inspector.call_end(context, inputs, outcome)
    }

    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut revm::interpreter::CreateInputs,
    ) -> Option<revm::interpreter::CreateOutcome> {
        self.gas_inspector.create(context, inputs)
    }

    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &revm::interpreter::CreateInputs,
        outcome: revm::interpreter::CreateOutcome,
    ) -> revm::interpreter::CreateOutcome {
        self.gas_inspector.create_end(context, inputs, outcome)
    }
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        self.gas_inspector.initialize_interp(interp, context);
    }
    fn log(&mut self, context: &mut EvmContext<DB>, log: &alloy_primitives::Log) {
        self.gas_inspector.log(context, log)
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
        let contract = Box::<Contract>::default();
        let mut interpreter = Interpreter::new(contract, 10000, false);
        let db = CacheDB::new(EmptyDB::default());

        let opcodes = [
            OpCode::new(opcode::ADD).unwrap(),
            OpCode::new(opcode::ADD).unwrap(),
            OpCode::new(opcode::ADD).unwrap(),
            OpCode::new(opcode::BYTE).unwrap(),
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
    #[test]
    fn test_with_variety_of_opcodes() {
        let mut opcode_counter = OpcodeCounterInspector::new();
        let contract = Box::<Contract>::default();
        let mut interpreter = Interpreter::new(contract, 2024, false);
        let db = CacheDB::new(EmptyDB::default());

        let opcodes = [
            opcode::PUSH1,
            opcode::PUSH1,
            opcode::ADD, 
            opcode::PUSH1,
            opcode::SSTORE, 
            opcode::STOP,  
        ];

        for opcode in opcodes.iter() {
            interpreter.instruction_pointer = opcode;
            opcode_counter.step(&mut interpreter, &mut EvmContext::new(db.clone()));
        }

        let opcode_counts = opcode_counter.opcode_counts();

        println!("{:?}", opcode_counts);

        let opcode_gas = opcode_counter.opcode_gas();
        println!("Opcode Gas Usage: {:?}", opcode_gas);
    }
}
