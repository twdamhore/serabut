//! Reporting module for PXE boot events.
//!
//! This module defines the `EventReporter` trait (ISP, DIP) and provides
//! implementations for different output formats.

mod console_reporter;

pub use console_reporter::ConsoleReporter;

use crate::domain::PxeBootEvent;

/// Trait for reporting PXE boot events (Interface Segregation Principle).
///
/// This trait is intentionally minimal - it only handles reporting,
/// not filtering or transformation. Different implementations can
/// output to console, files, databases, webhooks, etc.
pub trait EventReporter: Send {
    /// Report a PXE boot event.
    fn report(&self, event: &PxeBootEvent);

    /// Called when the listener starts.
    fn on_start(&self, interface: &str);

    /// Called when the listener stops.
    fn on_stop(&self);
}
