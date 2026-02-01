//! Timeout configuration for limiting EVM execution time.
//!
//! This module provides [`TimeoutConfig`] for configuring execution time limits
//! and external cancellation signals. It can be used with
//! [`TracingInspector`](crate::tracing::TracingInspector) and
//! [`JsInspector`](crate::tracing::js::JsInspector) via their timeout configuration methods.
//!
//! # Example
//!
//! ```rust,ignore
//! use revm_inspectors::timeout::TimeoutConfig;
//! use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};
//! use std::time::Duration;
//! use std::sync::Arc;
//! use std::sync::atomic::AtomicBool;
//!
//! // Create a timeout config
//! let timeout = TimeoutConfig::new(Duration::from_secs(5))
//!     .with_check_interval(1000);
//!
//! // Use with TracingInspector
//! let config = TracingInspectorConfig::default_geth();
//! let inspector = TracingInspector::new(config).with_timeout(timeout);
//!
//! // Or with an external cancellation signal
//! let cancel = Arc::new(AtomicBool::new(false));
//! let timeout = TimeoutConfig::new(Duration::from_secs(5))
//!     .with_signal(cancel.clone());
//!
//! // Cancel from another task/thread
//! cancel.store(true, std::sync::atomic::Ordering::Relaxed);
//! ```

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "std")]
use std::time::{Duration, Instant};

/// Configuration for execution timeout and cancellation.
///
/// This can be used to limit execution time and/or allow external cancellation
/// of EVM execution. Useful for preventing runaway execution when tracing with
/// overrides like max gas limit and zero gas price.
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Maximum duration for execution (requires std).
    #[cfg(feature = "std")]
    pub(crate) duration: Option<Duration>,
    /// External cancellation signal. When set to `true`, execution will be stopped.
    pub(crate) signal: Option<Arc<AtomicBool>>,
    /// Check interval: if `Some(n)`, check timeout every `n` opcodes during step.
    /// If `None`, only check at call/create boundaries.
    pub(crate) check_interval: Option<u64>,
}

#[cfg(feature = "std")]
impl TimeoutConfig {
    /// Create a new timeout configuration with the given duration.
    ///
    /// The execution will stop after the given duration has passed.
    pub fn new(duration: Duration) -> Self {
        Self { duration: Some(duration), signal: None, check_interval: None }
    }

    /// Get the configured duration.
    pub const fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

impl TimeoutConfig {
    /// Create a cancellation-only configuration without a timeout duration.
    ///
    /// This is useful in `no_std` environments or when you only need external
    /// cancellation without time-based limits.
    pub fn cancellation_only(signal: Arc<AtomicBool>) -> Self {
        Self {
            #[cfg(feature = "std")]
            duration: None,
            signal: Some(signal),
            check_interval: None,
        }
    }

    /// Set an external cancellation signal.
    ///
    /// The execution will be stopped when the signal is set to `true`.
    /// This is useful for cancelling execution from another task/thread,
    /// for example when a request is dropped.
    pub fn with_signal(mut self, signal: Arc<AtomicBool>) -> Self {
        self.signal = Some(signal);
        self
    }

    /// Set the opcode check interval.
    ///
    /// When set, the timeout/cancellation will also be checked every `n` opcodes
    /// during step execution. This is useful for long-running operations that
    /// don't make many calls.
    ///
    /// A value of `1` checks every opcode, `1000` checks every 1000 opcodes, etc.
    pub fn with_check_interval(mut self, interval: u64) -> Self {
        self.check_interval = Some(interval);
        self
    }

    /// Returns a reference to the external cancellation signal if set.
    pub fn signal(&self) -> Option<&Arc<AtomicBool>> {
        self.signal.as_ref()
    }

    /// Check if the external signal has been triggered.
    pub fn is_cancelled(&self) -> bool {
        self.signal.as_ref().is_some_and(|s| s.load(Ordering::Relaxed))
    }

    /// Get the check interval.
    pub const fn check_interval(&self) -> Option<u64> {
        self.check_interval
    }
}

/// Internal state for tracking timeout during execution.
#[derive(Debug, Default, Clone)]
pub(crate) struct TimeoutState {
    /// Execution start time (requires std).
    #[cfg(feature = "std")]
    execution_start: Option<Instant>,
    /// Counter for opcodes executed since last check.
    opcode_counter: u64,
}

impl TimeoutState {
    /// Reset the state for a new execution.
    pub(crate) fn reset(&mut self) {
        #[cfg(feature = "std")]
        {
            self.execution_start = Some(Instant::now());
        }
        self.opcode_counter = 0;
    }

    /// Check if execution has timed out.
    #[cfg(feature = "std")]
    pub(crate) fn has_timed_out(&self, config: &TimeoutConfig) -> bool {
        match (self.execution_start, config.duration) {
            (Some(start), Some(duration)) => start.elapsed() >= duration,
            _ => false,
        }
    }

    /// Check if execution should stop (timeout or cancellation).
    #[cfg(feature = "std")]
    pub(crate) fn should_stop(&self, config: &TimeoutConfig) -> bool {
        self.has_timed_out(config) || config.is_cancelled()
    }

    /// Check if execution should stop (cancellation only, for no_std).
    #[cfg(not(feature = "std"))]
    pub(crate) fn should_stop(&self, config: &TimeoutConfig) -> bool {
        config.is_cancelled()
    }

    /// Check if we should check timeout during step execution.
    /// Returns true if the interval counter has reached the check interval.
    pub(crate) fn should_check_step(&mut self, config: &TimeoutConfig) -> bool {
        if let Some(interval) = config.check_interval {
            self.opcode_counter += 1;
            if self.opcode_counter >= interval {
                self.opcode_counter = 0;
                return true;
            }
        }
        false
    }

    /// Get the timeout error message.
    #[cfg(feature = "std")]
    pub(crate) fn error_message(&self, config: &TimeoutConfig) -> &'static str {
        if self.has_timed_out(config) {
            "timeout during evm execution"
        } else {
            "execution cancelled"
        }
    }

    /// Get the timeout error message (no_std version).
    #[cfg(not(feature = "std"))]
    pub(crate) fn error_message(&self, _config: &TimeoutConfig) -> &'static str {
        "execution cancelled"
    }
}

/// Macro to check timeout at call/create boundaries and return early if triggered.
///
/// Use this in `call` and `create` inspector methods.
macro_rules! check_timeout {
    ($self:expr, $ctx:expr) => {
        if let Some(ref config) = $self.timeout_config {
            if $self.timeout_state.should_stop(config) {
                let msg = $self.timeout_state.error_message(config);
                *$ctx.error() =
                    Err(revm::context_interface::context::ContextError::Custom(msg.to_string()));
                return None;
            }
        }
    };
}

/// Macro to check timeout at call/create end boundaries.
///
/// Use this in `call_end` and `create_end` inspector methods.
macro_rules! check_timeout_end {
    ($self:expr, $ctx:expr) => {
        if let Some(ref config) = $self.timeout_config {
            if $self.timeout_state.should_stop(config) {
                let msg = $self.timeout_state.error_message(config);
                *$ctx.error() =
                    Err(revm::context_interface::context::ContextError::Custom(msg.to_string()));
            }
        }
    };
}

/// Macro to check timeout during step execution with interval checking.
///
/// Use this in `step` inspector methods.
macro_rules! check_timeout_step {
    ($self:expr, $ctx:expr) => {
        if let Some(ref config) = $self.timeout_config {
            if $self.timeout_state.should_check_step(config)
                && $self.timeout_state.should_stop(config)
            {
                let msg = $self.timeout_state.error_message(config);
                *$ctx.error() =
                    Err(revm::context_interface::context::ContextError::Custom(msg.to_string()));
                return;
            }
        }
    };
}

pub(crate) use check_timeout;
pub(crate) use check_timeout_end;
pub(crate) use check_timeout_step;

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_config_creation() {
        let config = TimeoutConfig::new(Duration::from_secs(5));
        assert_eq!(config.duration(), Some(Duration::from_secs(5)));
        assert!(config.signal().is_none());
        assert!(config.check_interval().is_none());
    }

    #[test]
    fn test_timeout_config_with_signal() {
        let signal = Arc::new(AtomicBool::new(false));
        let config = TimeoutConfig::new(Duration::from_secs(5)).with_signal(signal.clone());

        assert!(!config.is_cancelled());
        signal.store(true, Ordering::Relaxed);
        assert!(config.is_cancelled());
    }

    #[test]
    fn test_timeout_config_with_interval() {
        let config = TimeoutConfig::new(Duration::from_secs(5)).with_check_interval(1000);
        assert_eq!(config.check_interval(), Some(1000));
    }

    #[test]
    fn test_cancellation_only() {
        let signal = Arc::new(AtomicBool::new(false));
        let config = TimeoutConfig::cancellation_only(signal.clone());

        assert!(config.duration().is_none());
        assert!(!config.is_cancelled());
        signal.store(true, Ordering::Relaxed);
        assert!(config.is_cancelled());
    }

    #[test]
    fn test_timeout_state() {
        let config = TimeoutConfig::new(Duration::from_nanos(1));
        let mut state = TimeoutState::default();
        state.reset();
        std::thread::sleep(Duration::from_millis(1));
        assert!(state.has_timed_out(&config));
    }
}
