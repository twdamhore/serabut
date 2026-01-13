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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_pxe_info() -> PxeInfo {
        PxeInfo::from_vendor_class("PXEClient:Arch:00007:UNDI:003016").unwrap()
    }

    #[test]
    fn test_from_request_discover() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event =
            PxeBootEvent::from_request(mac, 0x12345678, DhcpMessageType::Discover, create_pxe_info());

        assert_eq!(event.client_mac, mac);
        assert_eq!(event.transaction_id, 0x12345678);
        assert_eq!(event.message_type, DhcpMessageType::Discover);
        assert!(event.assigned_ip.is_none());
        assert!(event.server_ip.is_none());
        assert!(event.is_client_request());
        assert!(!event.is_server_response());
    }

    #[test]
    fn test_from_request_request() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event =
            PxeBootEvent::from_request(mac, 0x12345678, DhcpMessageType::Request, create_pxe_info());

        assert!(event.is_client_request());
        assert!(!event.is_server_response());
    }

    #[test]
    fn test_from_reply_offer() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let assigned = Ipv4Addr::new(192, 168, 1, 100);
        let server = Ipv4Addr::new(192, 168, 1, 1);

        let event = PxeBootEvent::from_reply(
            mac,
            0x12345678,
            DhcpMessageType::Offer,
            assigned,
            server,
            create_pxe_info(),
        );

        assert_eq!(event.client_mac, mac);
        assert_eq!(event.transaction_id, 0x12345678);
        assert_eq!(event.message_type, DhcpMessageType::Offer);
        assert_eq!(event.assigned_ip, Some(assigned));
        assert_eq!(event.server_ip, Some(server));
        assert!(!event.is_client_request());
        assert!(event.is_server_response());
    }

    #[test]
    fn test_from_reply_ack() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let assigned = Ipv4Addr::new(192, 168, 1, 100);
        let server = Ipv4Addr::new(192, 168, 1, 1);

        let event = PxeBootEvent::from_reply(
            mac,
            0x12345678,
            DhcpMessageType::Ack,
            assigned,
            server,
            create_pxe_info(),
        );

        assert!(event.is_server_response());
        assert!(!event.is_client_request());
    }

    #[test]
    fn test_is_client_request_false_for_other_types() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);

        // Test with Release (not a request in our definition)
        let event =
            PxeBootEvent::from_request(mac, 0x12345678, DhcpMessageType::Release, create_pxe_info());
        assert!(!event.is_client_request());
        assert!(!event.is_server_response());
    }

    #[test]
    fn test_clone() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let event =
            PxeBootEvent::from_request(mac, 0x12345678, DhcpMessageType::Discover, create_pxe_info());

        let cloned = event.clone();

        assert_eq!(cloned.client_mac, event.client_mac);
        assert_eq!(cloned.transaction_id, event.transaction_id);
        assert_eq!(cloned.message_type, event.message_type);
    }

    #[test]
    fn test_timestamp_is_set() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let before = Instant::now();
        let event =
            PxeBootEvent::from_request(mac, 0x12345678, DhcpMessageType::Discover, create_pxe_info());
        let after = Instant::now();

        assert!(event.timestamp >= before);
        assert!(event.timestamp <= after);
    }

    #[test]
    fn test_pxe_info_preserved() {
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let pxe_info = create_pxe_info();
        let event =
            PxeBootEvent::from_request(mac, 0x12345678, DhcpMessageType::Discover, pxe_info);

        assert!(event.pxe_info.vendor_class.starts_with("PXEClient"));
    }
}
