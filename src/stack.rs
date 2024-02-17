use alloy_primitives::B256;
use std::fmt::Debug;

/// One can hook on inspector execution in 3 ways:
/// - Block: Hook on block execution
/// - BlockWithIndex: Hook on block execution transaction index
/// - Transaction: Hook on a specific transaction hash
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hook {
    #[default]
    /// No hook.
    None,
    /// Hook on a specific block.
    Block(u64),
    /// Hook on a specific transaction hash.
    Transaction(B256),
    /// Hooks on every transaction in a block.
    All,
}

/// Configuration for the inspectors.
#[derive(Clone, Copy, Debug, Default)]
pub struct InspectorStackConfig {
    /// Enable revm inspector printer.
    /// In execution this will print opcode level traces directly to console.
    pub use_printer_tracer: bool,

    /// Hook on a specific block or transaction.
    pub hook: Hook,
}

/// Helper macro to call the same method on multiple inspectors without resorting to dynamic
/// dispatch.
#[macro_export]
macro_rules! call_inspectors {
    ([$($inspector:expr),+ $(,)?], |$id:ident $(,)?| $call:expr $(,)?) => {{$(
        if let Some($id) = $inspector {
            $call
        }
    )+}}
}
