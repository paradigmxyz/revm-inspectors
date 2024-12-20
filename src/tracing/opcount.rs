//! Opcount tracing inspector that simply counts all opcodes.
//!
//! See also <https://geth.ethereum.org/docs/developers/evm-tracing/built-in-tracers>

use revm::{
    context_interface::Journal,
    interpreter::{interpreter::EthInterpreter, Interpreter},
    Context, Database,
};
use revm_inspector::Inspector;

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

impl<DB, BLOCK, TX, CFG, JOURNAL, CHAIN>
    Inspector<Context<BLOCK, TX, CFG, DB, JOURNAL, CHAIN>, EthInterpreter> for OpcodeCountInspector
where
    DB: Database,
    JOURNAL: Journal<Database = DB>,
{
    fn step(
        &mut self,
        _interp: &mut Interpreter<EthInterpreter>,
        _context: &mut Context<BLOCK, TX, CFG, DB, JOURNAL, CHAIN>,
    ) {
        self.count += 1;
    }
}
