//! Domain events for PXE boot monitoring.

use std::net::Ipv4Addr;
use std::time::Instant;

use macaddr::MacAddr6;

use super::pxe::PxeInfo;
use super::DhcpMessageType;

/// Represents a PXE boot event observed on the network.
///
/// This is the primary domain event that our system produces.
#[derive(Debug, Clone)]
pub struct PxeBootEvent {
    /// Timestamp when the event was observed
    pub timestamp: Instant,
    /// The MAC address of the PXE client
    pub client_mac: MacAddr6,
    /// The DHCP transaction ID
    pub transaction_id: u32,
    /// The type of DHCP message
    pub message_type: DhcpMessageType,
    /// IP address assigned to the client (from OFFER/ACK)
    pub assigned_ip: Option<Ipv4Addr>,
    /// The DHCP server that responded
    pub server_ip: Option<Ipv4Addr>,
    /// PXE-specific information
    pub pxe_info: PxeInfo,
}

impl PxeBootEvent {
    /// Create a new PXE boot event from a client request.
    pub fn from_request(
        client_mac: MacAddr6,
        transaction_id: u32,
        message_type: DhcpMessageType,
        pxe_info: PxeInfo,
    ) -> Self {
        Self {
            timestamp: Instant::now(),
            client_mac,
            transaction_id,
            message_type,
            assigned_ip: None,
            server_ip: None,
            pxe_info,
        }
    }

    /// Create a new PXE boot event from a server reply.
    pub fn from_reply(
        client_mac: MacAddr6,
        transaction_id: u32,
        message_type: DhcpMessageType,
        assigned_ip: Ipv4Addr,
        server_ip: Ipv4Addr,
        pxe_info: PxeInfo,
    ) -> Self {
        Self {
            timestamp: Instant::now(),
            client_mac,
            transaction_id,
            message_type,
            assigned_ip: Some(assigned_ip),
            server_ip: Some(server_ip),
            pxe_info,
        }
    }

    /// Check if this is a client request event.
    pub fn is_client_request(&self) -> bool {
        matches!(
            self.message_type,
            DhcpMessageType::Discover | DhcpMessageType::Request
        )
    }

    /// Check if this is a server response event.
    pub fn is_server_response(&self) -> bool {
        matches!(
            self.message_type,
            DhcpMessageType::Offer | DhcpMessageType::Ack
        )
    }
}
