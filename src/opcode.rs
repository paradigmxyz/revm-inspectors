use alloc::string::ToString;
use alloy_primitives::map::HashMap;
use alloy_rpc_types_trace::opcode::OpcodeGas;
use revm::{
    bytecode::opcode::{self, OpCode},
    interpreter::{
        interpreter_types::{Immediates, Jumps, LoopControl},
        Interpreter,
    },
    Inspector,
};

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

impl<CTX> Inspector<CTX> for OpcodeGasInspector {
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        let opcode_value = interp.bytecode.opcode();
        if let Some(opcode) = OpCode::new(opcode_value) {
            // keep track of opcode counts
            *self.opcode_counts.entry(opcode).or_default() += 1;

            // keep track of the last opcode executed
            self.last_opcode_gas_remaining = Some((opcode, interp.control.gas().remaining()));
        }
    }

    fn step_end(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        // update gas usage for the last opcode
        if let Some((opcode, gas_remaining)) = self.last_opcode_gas_remaining.take() {
            let gas_cost = gas_remaining.saturating_sub(interp.control.gas().remaining());
            *self.opcode_gas.entry(opcode).or_default() += gas_cost;
        }
    }
}

/// Accepts Bytecode that implements [Immediates] and returns the size of immediate
/// value.
///
/// Primarily needed to handle a special case of RJUMPV opcode.
pub fn immediate_size(bytecode: &impl Immediates) -> u8 {
    let opcode = bytecode.read_u8();
    if opcode == opcode::RJUMPV {
        let vtable_size = bytecode.read_slice(2)[2];
        return 1 + (vtable_size + 1) * 2;
    }
    let Some(opcode) = OpCode::new(opcode) else { return 0 };
    opcode.info().immediate_size()
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::{
        bytecode::Bytecode,
        database::CacheDB,
        database_interface::EmptyDB,
        interpreter::{interpreter::ExtBytecode, InputsImpl, SharedMemory},
        primitives::{hardfork::SpecId, Bytes},
        Context, MainContext,
    };
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn test_opcode_counter_inspector() {
        let mut opcode_counter = OpcodeGasInspector::new();

        let opcodes = [opcode::ADD, opcode::ADD, opcode::ADD, opcode::BYTE];

        let bytecode = Bytecode::new_raw(Bytes::from(opcodes));
        let mut interpreter = Interpreter::new(
            Rc::new(RefCell::new(SharedMemory::new())),
            ExtBytecode::new(bytecode),
            InputsImpl::default(),
            false,
            false,
            SpecId::LATEST,
            u64::MAX,
        );
        let db = CacheDB::new(EmptyDB::default());

        let mut context = Context::mainnet().with_db(db);
        for _ in &opcodes {
            opcode_counter.step(&mut interpreter, &mut context);
        }
    }

    #[test]
    fn test_with_variety_of_opcodes() {
        let mut opcode_counter = OpcodeGasInspector::new();

        let opcodes = [
            opcode::PUSH1,
            opcode::PUSH1,
            opcode::ADD,
            opcode::PUSH1,
            opcode::SSTORE,
            opcode::STOP,
        ];

        let bytecode = Bytecode::new_raw(Bytes::from(opcodes));
        let mut interpreter = Interpreter::new(
            Rc::new(RefCell::new(SharedMemory::new())),
            ExtBytecode::new(bytecode),
            InputsImpl::default(),
            false,
            false,
            SpecId::LATEST,
            u64::MAX,
        );
        let db = CacheDB::new(EmptyDB::default());

        let mut context = Context::mainnet().with_db(db);
        for _ in opcodes.iter() {
            opcode_counter.step(&mut interpreter, &mut context);
        }
    }
}
