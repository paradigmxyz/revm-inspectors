//! Recording limits and internal byte accounting.

use revm::{
    context_interface::{ContextError, ContextTr},
    interpreter::{interpreter_types::LoopControl, Interpreter},
};

/// Behavior when a trace recording reaches its byte budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TraceLimitBehavior {
    /// Continue execution but omit byte buffers that would exceed the limit.
    /// The resulting trace can contain empty fields. Check
    /// [`TracingInspector::limit_exceeded`](super::TracingInspector::limit_exceeded)
    /// to detect this.
    #[default]
    Skip,
    /// Abort execution with a revm error when a byte buffer would exceed the limit.
    Halt,
}

/// Limits applied when recording trace data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TraceLimits {
    /// Maximum cumulative byte-buffer allocation lengths between resets. `None` means unlimited.
    ///
    /// Counts copied call inputs, memory snapshots, immediate bytes and memory deltas.
    /// Shared buffer clones, stack/trace vector elements and allocator overhead are not counted.
    /// Checked before each byte-buffer allocation. Recording stops at this limit.
    pub max_recorded_bytes: Option<usize>,
    /// What to do when a recording would exceed `max_recorded_bytes`.
    pub behavior: TraceLimitBehavior,
}

impl TraceLimits {
    /// Sets the recorded-byte limit. `None` means unlimited.
    pub const fn set_max_recorded_bytes(mut self, max_recorded_bytes: Option<usize>) -> Self {
        self.max_recorded_bytes = max_recorded_bytes;
        self
    }

    /// Sets the behavior when a recording would exceed the budget.
    pub const fn set_behavior(mut self, behavior: TraceLimitBehavior) -> Self {
        self.behavior = behavior;
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
    pub(crate) fn reserve(&mut self, bytes: usize) -> bool {
        if self.exceeded
            || self.limits.max_recorded_bytes.is_some_and(|limit| {
                self.recorded.checked_add(bytes).is_none_or(|total| total > limit)
            })
        {
            self.exceeded = true;
            return false;
        }
        self.recorded += bytes;
        true
    }

    pub(crate) fn exceeded(&self) -> bool {
        self.exceeded
    }

    pub(crate) fn check(&self, context: &mut impl ContextTr) {
        if self.exceeded
            && self.limits.behavior == TraceLimitBehavior::Halt
            && context.error().is_ok()
        {
            *context.error() =
                Err(ContextError::Custom("trace recorded byte limit exceeded".into()));
        }
    }

    pub(crate) fn check_and_halt(&self, context: &mut impl ContextTr, interp: &mut Interpreter) {
        self.check(context);
        if self.exceeded && self.limits.behavior == TraceLimitBehavior::Halt {
            // Replace any pending CALL/RETURN action as well.
            interp.bytecode.action().take();
            interp.bytecode.reset_action();
            interp.halt_fatal();
        }
    }
}
