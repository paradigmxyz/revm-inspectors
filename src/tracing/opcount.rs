//! Opcount tracing inspector that simply counts all opcodes.
//!
//! See also <https://geth.ethereum.org/docs/developers/evm-tracing/built-in-tracers>

use revm::{interpreter::Interpreter, Database, EvmContext, Inspector};

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

impl<DB> Inspector<DB> for OpcodeCountInspector
where
    DB: Database,
{
    fn step(&mut self, _interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        self.count += 1;
    }
}
