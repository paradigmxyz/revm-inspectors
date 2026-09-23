//! Resource limits for recorded traces.

use thiserror::Error;

/// Limits on data recorded by a [`TracingInspector`](super::TracingInspector).
///
/// Limits are independent of tracing configuration and are never relaxed by config merging.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TraceLimits {
    /// Maximum cumulative bytes recorded between resets. `None` means unlimited.
    ///
    /// Charges Rust sizes for frame, step, log, ordering and storage records, plus the lengths
    /// of inputs, outputs, bytecode, log topics/data, stack snapshots, memory snapshots/deltas,
    /// return data and immediate bytes. Shared buffers are charged for each recorded occurrence;
    /// replacing data does not refund its previous charge.
    ///
    /// This is an accounting budget, not an exact heap or RPC response size limit. Allocator
    /// overhead, spare vector capacity, EVM state, caller mutations/decoding and result building
    /// are outside the budget. Execution continues after exhaustion, but recording stops and
    /// trace retrieval returns [`TraceError`].
    pub max_recorded_bytes: Option<usize>,
}

impl TraceLimits {
    /// Sets the cumulative recorded-byte limit. `None` means unlimited.
    pub const fn set_max_recorded_bytes(mut self, max_recorded_bytes: Option<usize>) -> Self {
        self.max_recorded_bytes = max_recorded_bytes;
        self
    }
}

/// A trace could not be recorded within its resource limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum TraceError {
    /// Recording the next item would exceed the byte budget.
    #[error(
        "trace byte limit exceeded (limit {limit}, recorded {recorded}, requested {requested})"
    )]
    LimitExceeded {
        /// Configured byte limit.
        limit: usize,
        /// Bytes charged before the rejected recording.
        recorded: usize,
        /// Bytes needed for the rejected recording.
        requested: usize,
    },
    /// The selected inspector cannot enforce this budget.
    #[error("recorded byte limits are not supported by this tracer")]
    UnsupportedTracer,
}

/// Cumulative, checked accounting with a sticky failure.
#[derive(Clone, Debug, Default)]
pub(crate) struct TraceBudget {
    pub(crate) limits: TraceLimits,
    pub(crate) recorded: usize,
    pub(crate) error: Option<TraceError>,
}

impl TraceBudget {
    pub(crate) fn record(&mut self, bytes: usize) -> bool {
        if self.error.is_some() {
            return false;
        }
        let limit = self.limits.max_recorded_bytes.unwrap_or(usize::MAX);
        if self.recorded.checked_add(bytes).is_none_or(|total| total > limit) {
            self.error = Some(TraceError::LimitExceeded {
                limit,
                recorded: self.recorded,
                requested: bytes,
            });
            return false;
        }
        self.recorded += bytes;
        true
    }

    pub(crate) fn check(&self) -> Result<(), TraceError> {
        self.error.map_or(Ok(()), Err)
    }

    pub(crate) fn reset(&mut self) {
        self.recorded = 0;
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_overflow_and_sticky_failure() {
        let mut budget = TraceBudget {
            limits: TraceLimits { max_recorded_bytes: Some(usize::MAX) },
            ..Default::default()
        };
        assert!(budget.record(usize::MAX));
        assert!(budget.record(0));
        assert!(!budget.record(1));
        let error = budget.check();
        assert!(!budget.record(0));
        assert_eq!(budget.check(), error);
        assert_eq!(budget.recorded, usize::MAX);
        budget.reset();
        assert_eq!(budget.limits.max_recorded_bytes, Some(usize::MAX));
        assert_eq!(budget.recorded, 0);
        assert!(budget.check().is_ok());
    }
}
