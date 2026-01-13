//! Packet capture abstraction.
//!
//! This module defines the `PacketCapture` trait (DIP) and provides
//! a pnet-based implementation. This allows for easy testing and
//! swapping implementations (OCP).

mod pnet_capture;

pub use pnet_capture::PnetCapture;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::CaptureError;

/// A raw network packet captured from the wire.
#[derive(Debug, Clone)]
pub struct RawPacket {
    /// The raw packet data
    pub data: Vec<u8>,
    /// Source MAC address
    pub src_mac: [u8; 6],
    /// Destination MAC address
    pub dst_mac: [u8; 6],
}

/// Trait for packet capture implementations (Dependency Inversion Principle).
///
/// This trait allows the application to depend on an abstraction rather
/// than a concrete implementation, making it easy to:
/// - Test with mock captures
/// - Switch between different capture backends (pnet, pcap, etc.)
/// - Replay captured packets from files
pub trait PacketCapture: Send {
    /// Start capturing packets and return an iterator over DHCP packets.
    ///
    /// Returns only the UDP payload of DHCP packets (ports 67/68).
    /// The iterator will return None when the running flag is set to false.
    fn capture_dhcp_packets(
        &mut self,
    ) -> Result<Box<dyn Iterator<Item = RawPacket> + '_>, CaptureError>;

    /// Get the name of the interface being captured.
    fn interface_name(&self) -> &str;

    /// Set the running flag for graceful shutdown.
    /// When set to false, the capture iterator should stop.
    fn set_running(&mut self, running: Arc<AtomicBool>);
}
