//! Serabut - DHCP PXE Boot Server
//!
//! A PXE boot server that monitors network traffic, downloads Ubuntu netboot
//! images, and serves them to PXE clients via proxyDHCP and TFTP.
//!
//! # Architecture
//!
//! This crate follows SOLID principles:
//!
//! - **S**ingle Responsibility: Each module has one job
//!   - `domain`: Core business types
//!   - `parser`: DHCP packet parsing
//!   - `capture`: Network packet capture
//!   - `detector`: PXE detection logic
//!   - `reporter`: Event output
//!   - `netboot`: Netboot image management
//!   - `tftp`: TFTP server for boot files
//!   - `proxydhcp`: ProxyDHCP server for PXE boot info
//!
//! - **O**pen/Closed: Extensible without modification
//!   - Add new reporters by implementing `EventReporter`
//!   - Add new capture backends by implementing `PacketCapture`
//!
//! - **L**iskov Substitution: Implementations are interchangeable
//!   - Any `PacketCapture` impl works with the listener
//!   - Any `EventReporter` impl works with the listener
//!
//! - **I**nterface Segregation: Minimal, focused traits
//!   - `PacketCapture` only handles capture
//!   - `EventReporter` only handles reporting
//!
//! - **D**ependency Inversion: Depend on abstractions
//!   - `PxeListener` depends on traits, not concrete types

pub mod capture;
pub mod detector;
pub mod domain;
pub mod error;
pub mod netboot;
pub mod parser;
pub mod proxydhcp;
pub mod reporter;
pub mod tftp;

use capture::{PacketCapture, RawPacket};
use detector::PxeDetector;
use parser::DhcpParser;
use reporter::EventReporter;

/// The main PXE boot listener.
///
/// Orchestrates packet capture, parsing, detection, and reporting
/// while depending only on abstractions (DIP).
pub struct PxeListener<C: PacketCapture, R: EventReporter> {
    capture: C,
    reporter: R,
    parser: DhcpParser,
    detector: PxeDetector,
}

impl<C: PacketCapture, R: EventReporter> PxeListener<C, R> {
    /// Create a new PXE listener with the given capture and reporter.
    pub fn new(capture: C, reporter: R) -> Self {
        Self {
            capture,
            reporter,
            parser: DhcpParser::new(),
            detector: PxeDetector::new(),
        }
    }

    /// Run the listener, processing packets until interrupted.
    pub fn run(&mut self) -> Result<(), error::AppError> {
        self.reporter.on_start(self.capture.interface_name());

        // Get references to processing components before starting capture
        // to avoid borrow checker issues with the mutable capture borrow
        let parser = &self.parser;
        let detector = &self.detector;
        let reporter = &self.reporter;

        let packets = self.capture.capture_dhcp_packets()?;

        for raw_packet in packets {
            Self::process_packet_with(parser, detector, reporter, &raw_packet);
        }

        self.reporter.on_stop();
        Ok(())
    }

    /// Process a single captured packet with explicit dependencies.
    fn process_packet_with(
        parser: &DhcpParser,
        detector: &PxeDetector,
        reporter: &R,
        raw: &RawPacket,
    ) {
        // Parse the DHCP packet
        let dhcp_packet = match parser.parse(&raw.data) {
            Ok(packet) => packet,
            Err(e) => {
                tracing::debug!("Failed to parse DHCP packet: {}", e);
                return;
            }
        };

        // Detect PXE boot activity
        if let Some(event) = detector.detect(&dhcp_packet) {
            reporter.report(&event);
        }
    }
}
