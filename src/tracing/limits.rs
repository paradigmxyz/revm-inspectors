//! Recording limits and internal byte accounting.

use revm::{
    context_interface::{ContextError, ContextTr},
    interpreter::{interpreter_types::LoopControl, Interpreter},
};

/// Limits applied when recording trace data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TraceLimits {
    /// Maximum cumulative byte-buffer allocation lengths between resets. `None` means unlimited.
    ///
    /// Counts copied call inputs, memory snapshots, immediate bytes and memory deltas.
    /// Shared buffer clones, stack/trace vector elements and allocator overhead are not counted.
    /// Checked after each recording callback, so the final callback can exceed the budget.
    /// Exceeding the budget aborts execution with a revm execution error.
    pub max_recorded_bytes: Option<usize>,
}

impl TraceLimits {
    /// Sets the recorded-byte limit. `None` means unlimited.
    pub const fn set_max_recorded_bytes(mut self, max_recorded_bytes: Option<usize>) -> Self {
        self.max_recorded_bytes = max_recorded_bytes;
        self
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TraceBudget {
    pub(crate) limits: TraceLimits,
    pub(crate) recorded: usize,
}

impl TraceBudget {
    pub(crate) fn record(&mut self, bytes: usize) {
        self.recorded = self.recorded.saturating_add(bytes);
    }

    pub(crate) fn exceeded(&self) -> bool {
        self.limits.max_recorded_bytes.is_some_and(|limit| self.recorded > limit)
    }

    pub(crate) fn check(&self, context: &mut impl ContextTr) {
        if self.exceeded() && context.error().is_ok() {
            *context.error() =
                Err(ContextError::Custom("trace recorded byte limit exceeded".into()));
        }
    }

    pub(crate) fn check_and_halt(&self, context: &mut impl ContextTr, interp: &mut Interpreter) {
        self.check(context);
        if self.exceeded() {
            // Replace any pending CALL/RETURN action as well.
            interp.bytecode.action().take();
            interp.bytecode.reset_action();
            interp.halt_fatal();
        }
    }
}
