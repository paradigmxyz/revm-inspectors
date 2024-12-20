//! Opcount tracing inspector that simply counts all opcodes.
//!
//! See also <https://geth.ethereum.org/docs/developers/evm-tracing/built-in-tracers>

use revm::{
    interpreter::{interpreter::EthInterpreter, Interpreter},
    Database,
};
use revm_inspector::{Inspector, PrevContext};

/// An inspector that counts all opcodes.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpcodeCountInspector {
    /// opcode counter
    count: usize,
}

impl OpcodeCountInspector {
    /// Returns the opcode counter
    #[inline]
    pub const fn count(&self) -> usize {
        self.count
    }
}

impl<DB> Inspector<PrevContext<DB>, EthInterpreter> for OpcodeCountInspector
where
    DB: Database,
{
    fn step(&mut self, _interp: &mut Interpreter<EthInterpreter>, _context: &mut PrevContext<DB>) {
        self.count += 1;
    }
}
