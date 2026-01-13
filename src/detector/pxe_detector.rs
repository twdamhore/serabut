//! PXE boot detection logic.

use crate::domain::{DhcpMessageType, DhcpPacket, PxeBootEvent, PxeInfo};

/// Detects PXE boot activity from DHCP packets.
///
/// Implements Single Responsibility Principle by focusing solely
/// on PXE detection logic, separate from parsing or reporting.
pub struct PxeDetector {
    /// Whether to include non-PXE DHCP traffic
    include_non_pxe: bool,
}

impl PxeDetector {
    /// Create a new PXE detector.
    pub fn new() -> Self {
        Self {
            include_non_pxe: false,
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
    pub fn detect(&self, packet: &DhcpPacket) -> Option<PxeBootEvent> {
        // Check if this is a PXE client by looking at vendor class ID
        let pxe_info = self.extract_pxe_info(packet)?;

        let message_type = packet.message_type()?;

        // Create the appropriate event based on message type
        match message_type {
            DhcpMessageType::Discover | DhcpMessageType::Request => {
                Some(PxeBootEvent::from_request(
                    packet.chaddr,
                    packet.xid,
                    message_type,
                    pxe_info,
                ))
            }
            DhcpMessageType::Offer | DhcpMessageType::Ack => {
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
        let mut options = vec![DhcpOption::MessageType(msg_type)];

        if let Some(vc) = vendor_class {
            options.push(DhcpOption::VendorClassId(vc.to_string()));
        }

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
    }

    #[test]
    fn test_non_pxe_ignored() {
        let detector = PxeDetector::new();
        let packet = create_test_packet(Some("MSFT 5.0"), DhcpMessageType::Discover);

        let event = detector.detect(&packet);
        assert!(event.is_none());
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
    }
}
