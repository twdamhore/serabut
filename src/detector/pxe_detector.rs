//! PXE boot detection logic.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use macaddr::MacAddr6;

use crate::domain::{DhcpMessageType, DhcpPacket, PxeBootEvent, PxeInfo};

/// Key for tracking PXE transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TransactionKey {
    xid: u32,
    mac: MacAddr6,
}

/// Stored information about a tracked PXE transaction.
#[derive(Debug, Clone)]
struct TrackedTransaction {
    pxe_info: PxeInfo,
    timestamp: Instant,
}

/// How long to keep tracked transactions before expiring them.
const TRANSACTION_TTL: Duration = Duration::from_secs(30);

/// Detects PXE boot activity from DHCP packets.
///
/// Implements Single Responsibility Principle by focusing solely
/// on PXE detection logic, separate from parsing or reporting.
///
/// Tracks PXE client requests (DISCOVER/REQUEST) so that corresponding
/// server responses (OFFER/ACK) can be matched even when the server
/// doesn't echo the PXE vendor class.
pub struct PxeDetector {
    /// Whether to include non-PXE DHCP traffic
    include_non_pxe: bool,
    /// Tracked PXE transactions (XID + MAC -> PxeInfo)
    transactions: Mutex<HashMap<TransactionKey, TrackedTransaction>>,
}

impl PxeDetector {
    /// Create a new PXE detector.
    pub fn new() -> Self {
        Self {
            include_non_pxe: false,
            transactions: Mutex::new(HashMap::new()),
        }
    }

    /// Configure whether to include non-PXE DHCP traffic.
    pub fn with_include_non_pxe(mut self, include: bool) -> Self {
        self.include_non_pxe = include;
        self
    }

    /// Analyze a DHCP packet and return a PXE boot event if detected.
    ///
    /// Returns `Some(PxeBootEvent)` if the packet is PXE-related,
    /// `None` otherwise.
    ///
    /// For client requests (DISCOVER/REQUEST) with PXE vendor class,
    /// the transaction is tracked so corresponding server responses
    /// can be detected even without PXE vendor class.
    pub fn detect(&self, packet: &DhcpPacket) -> Option<PxeBootEvent> {
        let message_type = packet.message_type()?;

        // Try to extract PXE info from the packet itself
        let pxe_info_from_packet = self.extract_pxe_info(packet);

        // Create transaction key for lookup/storage
        let key = TransactionKey {
            xid: packet.xid,
            mac: packet.chaddr,
        };

        // Create the appropriate event based on message type
        match message_type {
            DhcpMessageType::Discover | DhcpMessageType::Request => {
                // For client requests, we need PXE info from the packet
                let pxe_info = pxe_info_from_packet?;

                // Track this PXE transaction
                self.track_transaction(key, pxe_info.clone());

                Some(PxeBootEvent::from_request(
                    packet.chaddr,
                    packet.xid,
                    message_type,
                    pxe_info,
                ))
            }
            DhcpMessageType::Offer | DhcpMessageType::Ack => {
                // For server responses, try packet PXE info first,
                // then fall back to tracked transaction
                let pxe_info = pxe_info_from_packet
                    .or_else(|| self.lookup_transaction(&key))?;

                // For server responses, include the assigned IP
                let assigned_ip = if packet.yiaddr.is_unspecified() {
                    packet.ciaddr
                } else {
                    packet.yiaddr
                };

                Some(PxeBootEvent::from_reply(
                    packet.chaddr,
                    packet.xid,
                    message_type,
                    assigned_ip,
                    packet.siaddr,
                    pxe_info,
                ))
            }
            _ => None,
        }
    }

    /// Track a PXE transaction for later correlation with server responses.
    fn track_transaction(&self, key: TransactionKey, pxe_info: PxeInfo) {
        let mut transactions = self.transactions.lock().unwrap();

        // Clean up expired transactions while we have the lock
        let now = Instant::now();
        transactions.retain(|_, v| now.duration_since(v.timestamp) < TRANSACTION_TTL);

        // Store the new transaction
        transactions.insert(
            key,
            TrackedTransaction {
                pxe_info,
                timestamp: now,
            },
        );
    }

    /// Look up a tracked transaction by XID and MAC.
    fn lookup_transaction(&self, key: &TransactionKey) -> Option<PxeInfo> {
        let transactions = self.transactions.lock().unwrap();
        let tracked = transactions.get(key)?;

        // Check if the transaction is still valid
        if Instant::now().duration_since(tracked.timestamp) < TRANSACTION_TTL {
            Some(tracked.pxe_info.clone())
        } else {
            None
        }
    }

    /// Extract PXE information from a DHCP packet.
    fn extract_pxe_info(&self, packet: &DhcpPacket) -> Option<PxeInfo> {
        // Check for PXE vendor class ID (Option 60)
        let vendor_class = packet.vendor_class_id()?;

        // Must start with "PXEClient" to be a PXE request
        let mut pxe_info = PxeInfo::from_vendor_class(vendor_class)?;

        // Enhance with Option 93 (Client Architecture) if present
        if let Some(arch) = packet.client_arch() {
            pxe_info = pxe_info.with_architecture(arch);
        }

        // Add UUID if present (Option 97)
        if let Some(uuid) = packet.client_uuid() {
            pxe_info = pxe_info.with_uuid(uuid);
        }

        Some(pxe_info)
    }

    /// Check if a DHCP packet is from a PXE client.
    pub fn is_pxe_client(&self, packet: &DhcpPacket) -> bool {
        packet
            .vendor_class_id()
            .map(|vc| vc.starts_with("PXEClient"))
            .unwrap_or(false)
    }
}

impl Default for PxeDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DhcpOption;
    use macaddr::MacAddr6;
    use std::net::Ipv4Addr;

    fn create_test_packet(vendor_class: Option<&str>, msg_type: DhcpMessageType) -> DhcpPacket {
        create_test_packet_with_options(vendor_class, msg_type, vec![])
    }

    fn create_test_packet_with_options(
        vendor_class: Option<&str>,
        msg_type: DhcpMessageType,
        extra_options: Vec<DhcpOption>,
    ) -> DhcpPacket {
        let mut options = vec![DhcpOption::MessageType(msg_type)];

        if let Some(vc) = vendor_class {
            options.push(DhcpOption::VendorClassId(vc.to_string()));
        }

        options.extend(extra_options);

        DhcpPacket {
            op: 1,
            htype: 1,
            hlen: 6,
            xid: 0x12345678,
            secs: 0,
            flags: 0,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff),
            sname: None,
            file: None,
            options,
        }
    }

    fn create_reply_packet(
        vendor_class: Option<&str>,
        msg_type: DhcpMessageType,
        yiaddr: Ipv4Addr,
        siaddr: Ipv4Addr,
    ) -> DhcpPacket {
        let mut packet = create_test_packet(vendor_class, msg_type);
        packet.op = 2; // BOOTREPLY
        packet.yiaddr = yiaddr;
        packet.siaddr = siaddr;
        packet
    }

    #[test]
    fn test_detect_pxe_discover() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(
            Some("PXEClient:Arch:00000:UNDI:002001"),
            DhcpMessageType::Discover,
        );

        let event = detector.detect(&packet);
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Discover);
        assert!(event.pxe_info.vendor_class.starts_with("PXEClient"));
        assert!(event.assigned_ip.is_none());
        assert!(event.server_ip.is_none());
    }

    #[test]
    fn test_detect_pxe_request() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Request,
        );

        let event = detector.detect(&packet).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Request);
        assert!(event.is_client_request());
    }

    #[test]
    fn test_detect_pxe_offer() {
        let detector = PxeDetector::new();
        let packet = create_reply_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );

        let event = detector.detect(&packet).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Offer);
        assert_eq!(event.assigned_ip, Some(Ipv4Addr::new(192, 168, 1, 100)));
        assert_eq!(event.server_ip, Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(event.is_server_response());
    }

    #[test]
    fn test_detect_pxe_ack() {
        let detector = PxeDetector::new();
        let packet = create_reply_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Ack,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );

        let event = detector.detect(&packet).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Ack);
        assert!(event.is_server_response());
    }

    #[test]
    fn test_detect_uses_ciaddr_if_yiaddr_unspecified() {
        let detector = PxeDetector::new();
        let mut packet = create_reply_packet(
            Some("PXEClient"),
            DhcpMessageType::Ack,
            Ipv4Addr::UNSPECIFIED,
            Ipv4Addr::new(192, 168, 1, 1),
        );
        packet.ciaddr = Ipv4Addr::new(192, 168, 1, 50);

        let event = detector.detect(&packet).unwrap();
        // Should fall back to ciaddr when yiaddr is unspecified
        assert_eq!(event.assigned_ip, Some(Ipv4Addr::new(192, 168, 1, 50)));
    }

    #[test]
    fn test_non_pxe_ignored() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(Some("MSFT 5.0"), DhcpMessageType::Discover);

        let event = detector.detect(&packet);
        assert!(event.is_none());
    }

    #[test]
    fn test_no_vendor_class_ignored() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(None, DhcpMessageType::Discover);

        let event = detector.detect(&packet);
        assert!(event.is_none());
    }

    #[test]
    fn test_non_relevant_message_type_ignored() {
        let detector = PxeDetector::new();

        // DECLINE, NAK, RELEASE, INFORM should be ignored
        for msg_type in [
            DhcpMessageType::Decline,
            DhcpMessageType::Nak,
            DhcpMessageType::Release,
            DhcpMessageType::Inform,
        ] {
            let packet = create_test_packet(Some("PXEClient"), msg_type);
            assert!(
                detector.detect(&packet).is_none(),
                "Should ignore {:?}",
                msg_type
            );
        }
    }

    #[test]
    fn test_is_pxe_client() {
        let detector = PxeDetector::new();

        let pxe_packet = create_test_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Discover,
        );
        assert!(detector.is_pxe_client(&pxe_packet));

        let non_pxe_packet = create_test_packet(Some("MSFT 5.0"), DhcpMessageType::Discover);
        assert!(!detector.is_pxe_client(&non_pxe_packet));

        let no_vendor_packet = create_test_packet(None, DhcpMessageType::Discover);
        assert!(!detector.is_pxe_client(&no_vendor_packet));
    }

    #[test]
    fn test_is_pxe_client_case_sensitive() {
        let detector = PxeDetector::new();

        // Should be case-sensitive
        let lowercase = create_test_packet(Some("pxeclient"), DhcpMessageType::Discover);
        assert!(!detector.is_pxe_client(&lowercase));
    }

    #[test]
    fn test_architecture_from_option_93() {
        let detector = PxeDetector::new();
        let packet = create_test_packet_with_options(
            Some("PXEClient"),
            DhcpMessageType::Discover,
            vec![DhcpOption::ClientArch(7)],
        );

        let event = detector.detect(&packet).unwrap();
        assert_eq!(
            event.pxe_info.architecture,
            Some(crate::domain::PxeClientArch::EfiX64)
        );
    }

    #[test]
    fn test_uuid_from_option_97() {
        let detector = PxeDetector::new();
        let uuid_bytes = vec![
            0x00, // Type
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ];
        let packet = create_test_packet_with_options(
            Some("PXEClient"),
            DhcpMessageType::Discover,
            vec![DhcpOption::ClientUuid(uuid_bytes)],
        );

        let event = detector.detect(&packet).unwrap();
        assert!(event.pxe_info.uuid.is_some());
    }

    #[test]
    fn test_mac_address_captured() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(Some("PXEClient"), DhcpMessageType::Discover);

        let event = detector.detect(&packet).unwrap();
        assert_eq!(
            event.client_mac,
            MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff)
        );
    }

    #[test]
    fn test_transaction_id_captured() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(Some("PXEClient"), DhcpMessageType::Discover);

        let event = detector.detect(&packet).unwrap();
        assert_eq!(event.transaction_id, 0x12345678);
    }

    #[test]
    fn test_default_impl() {
        let detector = PxeDetector::default();
        let packet = create_test_packet(Some("PXEClient"), DhcpMessageType::Discover);
        assert!(detector.detect(&packet).is_some());
    }

    #[test]
    fn test_with_include_non_pxe() {
        let detector = PxeDetector::new().with_include_non_pxe(true);
        // This currently doesn't change behavior, but tests the builder pattern
        let packet = create_test_packet(Some("PXEClient"), DhcpMessageType::Discover);
        assert!(detector.detect(&packet).is_some());
    }

    #[test]
    fn test_minimal_pxe_client_string() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(Some("PXEClient"), DhcpMessageType::Discover);

        let event = detector.detect(&packet).unwrap();
        assert_eq!(event.pxe_info.vendor_class, "PXEClient");
        // No architecture parsed from minimal string
        assert!(event.pxe_info.architecture.is_none());
    }

    #[test]
    fn test_architecture_from_vendor_class() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(
            Some("PXEClient:Arch:00000:UNDI:002001"),
            DhcpMessageType::Discover,
        );

        let event = detector.detect(&packet).unwrap();
        assert_eq!(
            event.pxe_info.architecture,
            Some(crate::domain::PxeClientArch::IntelX86Bios)
        );
    }

    #[test]
    fn test_option_93_overrides_vendor_class_arch() {
        let detector = PxeDetector::new();
        // Vendor class says BIOS (0), but Option 93 says EFI x64 (7)
        let packet = create_test_packet_with_options(
            Some("PXEClient:Arch:00000:UNDI:002001"),
            DhcpMessageType::Discover,
            vec![DhcpOption::ClientArch(7)],
        );

        let event = detector.detect(&packet).unwrap();
        // Option 93 should take precedence
        assert_eq!(
            event.pxe_info.architecture,
            Some(crate::domain::PxeClientArch::EfiX64)
        );
    }

    #[test]
    fn test_no_message_type_option() {
        let detector = PxeDetector::new();

        // Create packet without message type option
        let packet = DhcpPacket {
            op: 1,
            htype: 1,
            hlen: 6,
            xid: 0x12345678,
            secs: 0,
            flags: 0,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff),
            sname: None,
            file: None,
            options: vec![DhcpOption::VendorClassId("PXEClient".to_string())],
        };

        // Should return None because message type is required
        assert!(detector.detect(&packet).is_none());
    }

    // Transaction tracking tests

    #[test]
    fn test_server_response_without_vendor_class_detected_via_tracking() {
        let detector = PxeDetector::new();
        let mac = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);
        let xid = 0x12345678;

        // First, send a PXE DISCOVER (with vendor class)
        let discover = create_test_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Discover,
        );
        let event = detector.detect(&discover);
        assert!(event.is_some());
        assert_eq!(event.unwrap().message_type, DhcpMessageType::Discover);

        // Now send a server OFFER without vendor class (standard DHCP response)
        let mut offer = create_reply_packet(
            None, // No PXE vendor class in server response
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );
        offer.xid = xid;
        offer.chaddr = mac;

        // Should still detect the OFFER because we tracked the DISCOVER
        let event = detector.detect(&offer);
        assert!(event.is_some(), "Server OFFER should be detected via transaction tracking");

        let event = event.unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Offer);
        assert_eq!(event.assigned_ip, Some(Ipv4Addr::new(192, 168, 1, 100)));
        assert_eq!(event.server_ip, Some(Ipv4Addr::new(192, 168, 1, 1)));
        // PXE info should come from the tracked transaction
        assert!(event.pxe_info.vendor_class.starts_with("PXEClient"));
    }

    #[test]
    fn test_server_ack_without_vendor_class_detected_via_tracking() {
        let detector = PxeDetector::new();

        // First, send a PXE REQUEST (with vendor class)
        let request = create_test_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Request,
        );
        detector.detect(&request);

        // Now send a server ACK without vendor class
        let ack = create_reply_packet(
            None,
            DhcpMessageType::Ack,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );

        let event = detector.detect(&ack);
        assert!(event.is_some(), "Server ACK should be detected via transaction tracking");
        assert_eq!(event.unwrap().message_type, DhcpMessageType::Ack);
    }

    #[test]
    fn test_untracked_server_response_ignored() {
        let detector = PxeDetector::new();

        // Send a server OFFER without any prior PXE request
        let offer = create_reply_packet(
            None,
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );

        // Should NOT detect because there's no tracked PXE transaction
        let event = detector.detect(&offer);
        assert!(event.is_none(), "Untracked server response should be ignored");
    }

    #[test]
    fn test_different_xid_not_matched() {
        let detector = PxeDetector::new();

        // Send a PXE DISCOVER with one XID
        let mut discover = create_test_packet(
            Some("PXEClient"),
            DhcpMessageType::Discover,
        );
        discover.xid = 0x11111111;
        detector.detect(&discover);

        // Send a server OFFER with different XID
        let mut offer = create_reply_packet(
            None,
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );
        offer.xid = 0x22222222;

        // Should NOT detect because XIDs don't match
        let event = detector.detect(&offer);
        assert!(event.is_none(), "Response with different XID should not be matched");
    }

    #[test]
    fn test_different_mac_not_matched() {
        let detector = PxeDetector::new();

        // Send a PXE DISCOVER with one MAC
        let mut discover = create_test_packet(
            Some("PXEClient"),
            DhcpMessageType::Discover,
        );
        discover.chaddr = MacAddr6::new(0x11, 0x22, 0x33, 0x44, 0x55, 0x66);
        detector.detect(&discover);

        // Send a server OFFER with same XID but different MAC
        let mut offer = create_reply_packet(
            None,
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 1),
        );
        offer.chaddr = MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff);

        // Should NOT detect because MACs don't match
        let event = detector.detect(&offer);
        assert!(event.is_none(), "Response with different MAC should not be matched");
    }

    #[test]
    fn test_full_pxe_exchange_sequence() {
        let detector = PxeDetector::new();
        let mac = MacAddr6::new(0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe);
        let xid = 0xaabbccdd;

        // 1. Client sends DISCOVER
        let mut discover = create_test_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Discover,
        );
        discover.xid = xid;
        discover.chaddr = mac;
        let event = detector.detect(&discover).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Discover);
        assert!(event.pxe_info.architecture.is_some());

        // 2. Server sends OFFER (no vendor class)
        let mut offer = create_reply_packet(
            None,
            DhcpMessageType::Offer,
            Ipv4Addr::new(10, 0, 0, 50),
            Ipv4Addr::new(10, 0, 0, 1),
        );
        offer.xid = xid;
        offer.chaddr = mac;
        let event = detector.detect(&offer).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Offer);
        assert_eq!(event.assigned_ip, Some(Ipv4Addr::new(10, 0, 0, 50)));
        // Architecture should be preserved from the tracked DISCOVER
        assert!(event.pxe_info.architecture.is_some());

        // 3. Client sends REQUEST
        let mut request = create_test_packet(
            Some("PXEClient:Arch:00007:UNDI:003016"),
            DhcpMessageType::Request,
        );
        request.xid = xid;
        request.chaddr = mac;
        let event = detector.detect(&request).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Request);

        // 4. Server sends ACK (no vendor class)
        let mut ack = create_reply_packet(
            None,
            DhcpMessageType::Ack,
            Ipv4Addr::new(10, 0, 0, 50),
            Ipv4Addr::new(10, 0, 0, 1),
        );
        ack.xid = xid;
        ack.chaddr = mac;
        let event = detector.detect(&ack).unwrap();
        assert_eq!(event.message_type, DhcpMessageType::Ack);
        assert_eq!(event.assigned_ip, Some(Ipv4Addr::new(10, 0, 0, 50)));
    }

    #[test]
    fn test_multiple_concurrent_transactions() {
        let detector = PxeDetector::new();

        // Client 1 DISCOVER
        let mut discover1 = create_test_packet(
            Some("PXEClient:Arch:00007"),
            DhcpMessageType::Discover,
        );
        discover1.xid = 0x11111111;
        discover1.chaddr = MacAddr6::new(0x11, 0x11, 0x11, 0x11, 0x11, 0x11);
        detector.detect(&discover1);

        // Client 2 DISCOVER
        let mut discover2 = create_test_packet(
            Some("PXEClient:Arch:00000"),
            DhcpMessageType::Discover,
        );
        discover2.xid = 0x22222222;
        discover2.chaddr = MacAddr6::new(0x22, 0x22, 0x22, 0x22, 0x22, 0x22);
        detector.detect(&discover2);

        // Server responds to Client 2 first
        let mut offer2 = create_reply_packet(
            None,
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 102),
            Ipv4Addr::new(192, 168, 1, 1),
        );
        offer2.xid = 0x22222222;
        offer2.chaddr = MacAddr6::new(0x22, 0x22, 0x22, 0x22, 0x22, 0x22);
        let event2 = detector.detect(&offer2).unwrap();
        assert_eq!(event2.assigned_ip, Some(Ipv4Addr::new(192, 168, 1, 102)));

        // Server responds to Client 1
        let mut offer1 = create_reply_packet(
            None,
            DhcpMessageType::Offer,
            Ipv4Addr::new(192, 168, 1, 101),
            Ipv4Addr::new(192, 168, 1, 1),
        );
        offer1.xid = 0x11111111;
        offer1.chaddr = MacAddr6::new(0x11, 0x11, 0x11, 0x11, 0x11, 0x11);
        let event1 = detector.detect(&offer1).unwrap();
        assert_eq!(event1.assigned_ip, Some(Ipv4Addr::new(192, 168, 1, 101)));
    }
}
