//! Opcount tracing inspector that simply counts all opcodes.
//!
//! See also <https://geth.ethereum.org/docs/developers/evm-tracing/built-in-tracers>

use revm::{
    handler::Inspector,
    interpreter::{interpreter::EthInterpreter, Interpreter},
};

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

impl<CTX> Inspector<CTX, EthInterpreter> for OpcodeCountInspector {
    fn step(&mut self, _interp: &mut Interpreter<EthInterpreter>, _context: &mut CTX) {
        self.count += 1;
    }
}
