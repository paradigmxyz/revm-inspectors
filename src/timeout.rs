//! Configurable timeout inspector for limiting EVM execution time.
//!
//! This module provides a [`TimeoutInspector`] that can be used to limit the wall-clock time
//! spent on EVM execution. It supports:
//!
//! - Duration-based timeouts (check if elapsed time exceeds a limit)
//! - Configurable check intervals (check every N opcodes instead of every step)
//! - External cancellation via an [`AtomicBool`] signal
//!
//! # Example
//!
//! ```rust,ignore
//! use revm_inspectors::timeout::TimeoutInspector;
//! use std::time::Duration;
//! use std::sync::Arc;
//! use std::sync::atomic::AtomicBool;
//!
//! // Create a timeout inspector that checks every 1000 opcodes
//! let inspector = TimeoutInspector::new(Duration::from_secs(5))
//!     .with_check_interval(1000);
//!
//! // Or with an external cancellation signal
//! let cancel = Arc::new(AtomicBool::new(false));
//! let inspector = TimeoutInspector::new(Duration::from_secs(5))
//!     .with_signal(cancel.clone())
//!     .with_check_interval(1000);
//!
//! // Cancel from another task/thread
//! cancel.store(true, std::sync::atomic::Ordering::Relaxed);
//!
//! // Or cancellation-only (no timeout)
//! let inspector = TimeoutInspector::cancellation_only(cancel.clone());
//! ```

use alloc::{string::ToString, sync::Arc};
use core::sync::atomic::{AtomicBool, Ordering};
use revm::{
    context_interface::{context::ContextError, ContextTr},
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
    Inspector,
};

#[cfg(feature = "std")]
use std::time::{Duration, Instant};

/// A revm [`Inspector`] that limits execution time and supports external cancellation.
///
/// This inspector will stop execution when:
/// - The configured duration has elapsed (requires `std` feature)
/// - An external cancellation signal is received via [`AtomicBool`]
///
/// ## Check Points
///
/// The timeout/cancellation is checked at:
/// - The start of each call (`call`, `create`)
/// - The end of each call (`call_end`, `create_end`)
/// - During step execution if `check_interval` is configured
///
/// ## Usage Note
///
/// When the timeout is triggered, it will set a [`ContextError::Custom`] error.
/// To avoid inspecting invalid data, this inspector should be the OUTERMOST inspector
/// in any multi-inspector setup.
///
/// ## no_std Support
///
/// When compiled without `std`, only the cancellation signal functionality is available.
/// Use [`TimeoutInspector::cancellation_only`] to create an inspector that only checks
/// the external signal.
#[derive(Debug)]
pub struct TimeoutInspector {
    /// Maximum duration for execution (requires std).
    #[cfg(feature = "std")]
    duration: Option<Duration>,
    /// Execution start time (requires std).
    #[cfg(feature = "std")]
    execution_start: Instant,
    /// External cancellation signal. When set to `true`, execution will be stopped.
    signal: Option<Arc<AtomicBool>>,
    /// Check interval: if `Some(n)`, only check during step every `n` opcodes.
    /// If `None`, only check at call/create boundaries.
    check_interval: Option<u64>,
    /// Counter for opcodes executed since last check.
    opcode_counter: u64,
}

#[cfg(feature = "std")]
impl TimeoutInspector {
    /// Create a new timeout inspector with the given duration.
    ///
    /// The inspector will stop execution after the given duration has passed.
    pub fn new(duration: Duration) -> Self {
        Self {
            duration: Some(duration),
            execution_start: Instant::now(),
            signal: None,
            check_interval: None,
            opcode_counter: 0,
        }
    }

    /// Check if the timeout has been reached.
    pub fn has_timed_out(&self) -> bool {
        self.duration.is_some_and(|d| self.execution_start.elapsed() >= d)
    }

    /// Get the elapsed time since execution started.
    pub fn elapsed(&self) -> Duration {
        self.execution_start.elapsed()
    }

    /// Get the configured duration.
    pub const fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

impl TimeoutInspector {
    /// Create a cancellation-only inspector without a timeout duration.
    ///
    /// This is useful in `no_std` environments or when you only need external
    /// cancellation without time-based limits.
    pub fn cancellation_only(signal: Arc<AtomicBool>) -> Self {
        Self {
            #[cfg(feature = "std")]
            duration: None,
            #[cfg(feature = "std")]
            execution_start: Instant::now(),
            signal: Some(signal),
            check_interval: None,
            opcode_counter: 0,
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

    /// Set the external cancellation signal.
    pub fn set_signal(&mut self, signal: Arc<AtomicBool>) {
        self.signal = Some(signal);
    }

    /// Returns a reference to the external cancellation signal if set.
    pub fn signal(&self) -> Option<&Arc<AtomicBool>> {
        self.signal.as_ref()
    }

    /// Check if the external signal has been triggered.
    pub fn is_cancelled(&self) -> bool {
        self.signal.as_ref().is_some_and(|s| s.load(Ordering::Relaxed))
    }

    /// Check if execution should stop (either timeout or cancellation).
    pub fn should_stop(&self) -> bool {
        #[cfg(feature = "std")]
        if self.has_timed_out() {
            return true;
        }
        self.is_cancelled()
    }

    /// Reset the execution state. Called during `initialize_interp`.
    pub fn reset(&mut self) {
        #[cfg(feature = "std")]
        {
            self.execution_start = Instant::now();
        }
        self.opcode_counter = 0;
    }

    /// Get the check interval.
    pub const fn check_interval(&self) -> Option<u64> {
        self.check_interval
    }

    /// Check timeout/cancellation and set error if triggered.
    #[inline]
    fn check_and_set_error<CTX>(&self, ctx: &mut CTX)
    where
        CTX: ContextTr,
    {
        #[cfg(feature = "std")]
        if self.has_timed_out() {
            *ctx.error() = Err(ContextError::Custom("timeout during evm execution".to_string()));
            return;
        }
        if self.is_cancelled() {
            *ctx.error() = Err(ContextError::Custom("execution cancelled".to_string()));
        }
    }

    /// Check timeout during step execution, respecting the check interval.
    #[inline]
    fn check_step_timeout<CTX>(&mut self, ctx: &mut CTX)
    where
        CTX: ContextTr,
    {
        if let Some(interval) = self.check_interval {
            self.opcode_counter += 1;
            if self.opcode_counter >= interval {
                self.opcode_counter = 0;
                self.check_and_set_error(ctx);
            }
        }
    }
}

impl<CTX> Inspector<CTX> for TimeoutInspector
where
    CTX: ContextTr,
{
    fn initialize_interp(&mut self, _interp: &mut Interpreter, _ctx: &mut CTX) {
        self.reset();
    }

    fn step(&mut self, _interp: &mut Interpreter, ctx: &mut CTX) {
        self.check_step_timeout(ctx);
    }

    fn call(&mut self, ctx: &mut CTX, _inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.check_and_set_error(ctx);
        None
    }

    fn call_end(&mut self, ctx: &mut CTX, _inputs: &CallInputs, _outcome: &mut CallOutcome) {
        self.check_and_set_error(ctx);
    }

    fn create(&mut self, ctx: &mut CTX, _inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        self.check_and_set_error(ctx);
        None
    }

    fn create_end(&mut self, ctx: &mut CTX, _inputs: &CreateInputs, _outcome: &mut CreateOutcome) {
        self.check_and_set_error(ctx);
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_inspector_creation() {
        let inspector = TimeoutInspector::new(Duration::from_secs(5));
        assert!(!inspector.has_timed_out());
        assert!(!inspector.is_cancelled());
        assert!(!inspector.should_stop());
        assert_eq!(inspector.duration(), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_timeout_inspector_with_signal() {
        let signal = Arc::new(AtomicBool::new(false));
        let inspector = TimeoutInspector::new(Duration::from_secs(5)).with_signal(signal.clone());

        assert!(!inspector.is_cancelled());
        signal.store(true, Ordering::Relaxed);
        assert!(inspector.is_cancelled());
        assert!(inspector.should_stop());
    }

    #[test]
    fn test_timeout_inspector_with_interval() {
        let inspector = TimeoutInspector::new(Duration::from_secs(5)).with_check_interval(1000);
        assert_eq!(inspector.check_interval(), Some(1000));
    }

    #[test]
    fn test_cancellation_only() {
        let signal = Arc::new(AtomicBool::new(false));
        let inspector = TimeoutInspector::cancellation_only(signal.clone());

        assert!(inspector.duration().is_none());
        assert!(!inspector.is_cancelled());
        signal.store(true, Ordering::Relaxed);
        assert!(inspector.is_cancelled());
        assert!(inspector.should_stop());
    }

    #[test]
    fn test_timeout_immediate() {
        let inspector = TimeoutInspector::new(Duration::from_nanos(1));
        std::thread::sleep(Duration::from_millis(1));
        assert!(inspector.has_timed_out());
    }
}
