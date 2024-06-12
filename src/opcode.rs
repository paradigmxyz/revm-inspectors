use alloy_rpc_types::trace::opcode::OpcodeGas;
use revm::{
    interpreter::{opcode::OpCode, Interpreter},
    Database, EvmContext, Inspector,
};
use std::collections::HashMap;

/// An Inspector that counts opcodes and measures gas usage per opcode.
#[derive(Clone, Debug, Default)]
pub struct OpcodeGasInspector {
    /// Map of opcode counts per transaction.
    opcode_counts: HashMap<OpCode, u64>,
    /// Map of total gas used per opcode.
    opcode_gas: HashMap<OpCode, u64>,
    /// Keep track of the last opcode executed and the remaining gas
    last_opcode_gas_remaining: Option<(OpCode, u64)>,
}

impl OpcodeGasInspector {
    /// Creates a new instance of the inspector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the opcode counts collected during transaction execution.
    pub const fn opcode_counts(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_counts
    }

    /// Returns the opcode gas usage collected during transaction execution.
    pub const fn opcode_gas(&self) -> &HashMap<OpCode, u64> {
        &self.opcode_gas
    }

    /// Returns an iterator over all opcodes with their count and combined gas usage.
    ///
    /// Note: this returns in no particular order.
    pub fn opcode_iter(&self) -> impl Iterator<Item = (OpCode, (u64, u64))> + '_ {
        self.opcode_counts.iter().map(move |(&opcode, &count)| {
            let gas = self.opcode_gas.get(&opcode).copied().unwrap_or_default();
            (opcode, (count, gas))
        })
    }

    /// Returns an iterator over all opcodes with their count and combined gas usage.
    ///
    /// Note: this returns in no particular order.
    pub fn opcode_gas_iter(&self) -> impl Iterator<Item = OpcodeGas> + '_ {
        self.opcode_iter().map(|(opcode, (count, gas_used))| OpcodeGas {
            opcode: opcode.to_string(),
            count,
            gas_used,
        })
    }
}

impl<DB> Inspector<DB> for OpcodeGasInspector
where
    DB: Database,
{
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let opcode_value = interp.current_opcode();
        if let Some(opcode) = OpCode::new(opcode_value) {
            // keep track of opcode counts
            *self.opcode_counts.entry(opcode).or_default() += 1;

            // keep track of the last opcode executed
            self.last_opcode_gas_remaining = Some((opcode, interp.gas().remaining()));
        }
    }

    fn step_end(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        // update gas usage for the last opcode
        if let Some((opcode, gas_remaining)) = self.last_opcode_gas_remaining.take() {
            let gas_cost = gas_remaining.saturating_sub(interp.gas().remaining());
            *self.opcode_gas.entry(opcode).or_default() += gas_cost;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::{
        db::{CacheDB, EmptyDB},
        interpreter::{opcode, Contract},
    };

    #[test]
    fn test_opcode_counter_inspector() {
        let mut opcode_counter = OpcodeGasInspector::new();
        let contract = Contract::default();
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
    }

    #[test]
    fn test_with_variety_of_opcodes() {
        let mut opcode_counter = OpcodeGasInspector::new();
        let contract = Contract::default();
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
    }
}
