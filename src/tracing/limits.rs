//! Internal recording budget for the tracing inspector.

use revm::context_interface::{ContextError, ContextTr};

/// Limits applied when recording trace data, independently of tracing configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TraceLimits {
    /// Maximum cumulative recorded bytes between resets. `None` means unlimited.
    ///
    /// Counts frame/step metadata and recorded inputs, outputs, bytecode, logs, stack,
    /// memory, storage, return data and immediate bytes. Shared buffers count per recording.
    /// This is not exact heap accounting: spare capacity, allocator overhead, EVM state,
    /// caller mutations and result building are excluded.
    ///
    /// Exceeding the budget aborts inspection with a revm execution error. Previously
    /// recorded data remains accessible through the existing trace accessors.
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
    pub(crate) exceeded: bool,
}

impl TraceBudget {
    pub(crate) fn record(&mut self, bytes: usize) -> bool {
        let limit = self.limits.max_recorded_bytes.unwrap_or(usize::MAX);
        self.exceeded |= self.recorded.checked_add(bytes).is_none_or(|total| total > limit);
        if self.exceeded {
            return false;
        }
        self.recorded += bytes;
        true
    }

    pub(crate) fn propagate_error(&self, context: &mut impl ContextTr) -> bool {
        if self.exceeded && context.error().is_ok() {
            *context.error() =
                Err(ContextError::Custom("trace recorded byte limit exceeded".into()));
        }
        self.exceeded
    }

    pub(crate) fn reset(&mut self) {
        self.recorded = 0;
        self.exceeded = false;
    }
}
