//! DHCP packet parser implementation.
//!
//! Parses raw DHCP packets according to RFC 2131.

use std::net::Ipv4Addr;

use macaddr::MacAddr6;

use crate::domain::{DhcpMessageType, DhcpOption, DhcpPacket};
use crate::error::ParseError;

/// DHCP magic cookie: 0x63825363
const DHCP_MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

/// Minimum DHCP packet size (without options)
const MIN_DHCP_SIZE: usize = 236;

/// DHCP option codes
mod option_codes {
    pub const PAD: u8 = 0;
    pub const END: u8 = 255;
    pub const MESSAGE_TYPE: u8 = 53;
    pub const REQUESTED_IP: u8 = 50;
    pub const SERVER_ID: u8 = 54;
    pub const VENDOR_CLASS_ID: u8 = 60;
    pub const CLIENT_ID: u8 = 61;
    pub const CLIENT_ARCH: u8 = 93;
    pub const CLIENT_NDI: u8 = 94;
    pub const CLIENT_UUID: u8 = 97;
}

/// Parser for DHCP packets.
///
/// Implements the Single Responsibility Principle by focusing
/// solely on parsing DHCP wire format into domain types.
pub struct DhcpParser;

impl DhcpParser {
    /// Create a new DHCP parser.
    pub fn new() -> Self {
        Self
    }

    /// Parse a DHCP packet from raw bytes.
    ///
    /// The input should be the UDP payload (not including IP/UDP headers).
    pub fn parse(&self, data: &[u8]) -> Result<DhcpPacket, ParseError> {
        if data.len() < MIN_DHCP_SIZE {
            return Err(ParseError::PacketTooShort {
                expected: MIN_DHCP_SIZE,
                actual: data.len(),
            });
        }

        // Parse fixed header fields
        let op = data[0];
        let htype = data[1];
        let hlen = data[2];
        // hops at [3]
        let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let secs = u16::from_be_bytes([data[8], data[9]]);
        let flags = u16::from_be_bytes([data[10], data[11]]);

        let ciaddr = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
        let yiaddr = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
        let siaddr = Ipv4Addr::new(data[20], data[21], data[22], data[23]);
        let giaddr = Ipv4Addr::new(data[24], data[25], data[26], data[27]);

        // Client hardware address (chaddr) - 16 bytes starting at offset 28
        // We always extract first 6 bytes as MAC (works for Ethernet htype=1, hlen=6)
        let chaddr = MacAddr6::new(
            data[28], data[29], data[30], data[31], data[32], data[33],
        );

        // Server name (sname) - 64 bytes starting at offset 44
        let sname = Self::parse_null_terminated_string(&data[44..108]);

        // Boot filename (file) - 128 bytes starting at offset 108
        let file = Self::parse_null_terminated_string(&data[108..236]);

        // Check for DHCP magic cookie at offset 236
        if data.len() < 240 {
            return Err(ParseError::PacketTooShort {
                expected: 240,
                actual: data.len(),
            });
        }

        if data[236..240] != DHCP_MAGIC_COOKIE {
            return Err(ParseError::InvalidMagicCookie);
        }

        // Parse options starting at offset 240
        let options = self.parse_options(&data[240..])?;

        Ok(DhcpPacket {
            op,
            htype,
            hlen,
            xid,
            secs,
            flags,
            ciaddr,
            yiaddr,
            siaddr,
            giaddr,
            chaddr,
            sname,
            file,
            options,
        })
    }

    /// Parse a null-terminated string, returning None if empty.
    fn parse_null_terminated_string(data: &[u8]) -> Option<String> {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        if end == 0 {
            return None;
        }

        String::from_utf8(data[..end].to_vec()).ok()
    }

    /// Parse DHCP options from the options section.
    fn parse_options(&self, data: &[u8]) -> Result<Vec<DhcpOption>, ParseError> {
        let mut options = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let code = data[offset];

            // Handle special codes
            if code == option_codes::PAD {
                offset += 1;
                continue;
            }

            if code == option_codes::END {
                break;
            }

            // Regular option: code + length + data
            if offset + 1 >= data.len() {
                return Err(ParseError::InvalidOption {
                    offset,
                    message: "option length missing".to_string(),
                });
            }

            let len = data[offset + 1] as usize;

            if offset + 2 + len > data.len() {
                return Err(ParseError::InvalidOption {
                    offset,
                    message: format!(
                        "option data truncated: expected {} bytes, have {}",
                        len,
                        data.len() - offset - 2
                    ),
                });
            }

            let option_data = &data[offset + 2..offset + 2 + len];

            if let Some(option) = self.parse_option(code, option_data) {
                options.push(option);
            }

            offset += 2 + len;
        }

        Ok(options)
    }

    /// Parse a single DHCP option.
    fn parse_option(&self, code: u8, data: &[u8]) -> Option<DhcpOption> {
        match code {
            option_codes::MESSAGE_TYPE => {
                if data.is_empty() {
                    return None;
                }
                DhcpMessageType::from_u8(data[0]).map(DhcpOption::MessageType)
            }

            option_codes::REQUESTED_IP => {
                if data.len() < 4 {
                    return None;
                }
                Some(DhcpOption::RequestedIp(Ipv4Addr::new(
                    data[0], data[1], data[2], data[3],
                )))
            }

            option_codes::SERVER_ID => {
                if data.len() < 4 {
                    return None;
                }
                Some(DhcpOption::ServerIdentifier(Ipv4Addr::new(
                    data[0], data[1], data[2], data[3],
                )))
            }

            option_codes::VENDOR_CLASS_ID => {
                String::from_utf8(data.to_vec())
                    .ok()
                    .map(DhcpOption::VendorClassId)
            }

            option_codes::CLIENT_ID => Some(DhcpOption::ClientId(data.to_vec())),

            option_codes::CLIENT_ARCH => {
                if data.len() < 2 {
                    return None;
                }
                Some(DhcpOption::ClientArch(u16::from_be_bytes([
                    data[0], data[1],
                ])))
            }

            option_codes::CLIENT_NDI => Some(DhcpOption::ClientNdi(data.to_vec())),

            option_codes::CLIENT_UUID => Some(DhcpOption::ClientUuid(data.to_vec())),

            _ => Some(DhcpOption::Unknown(code, data.to_vec())),
        }
    }
}

impl Default for DhcpParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimum_packet() {
        let parser = DhcpParser::new();

        // Create a minimal valid DHCP packet
        let mut packet = vec![0u8; 300];

        // Op: BOOTREQUEST
        packet[0] = 1;
        // Hardware type: Ethernet
        packet[1] = 1;
        // Hardware address length
        packet[2] = 6;
        // XID
        packet[4..8].copy_from_slice(&0x12345678u32.to_be_bytes());
        // MAC address
        packet[28..34].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        // Magic cookie
        packet[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        // Message type option (DISCOVER)
        packet[240] = option_codes::MESSAGE_TYPE;
        packet[241] = 1;
        packet[242] = 1; // DISCOVER
        // End option
        packet[243] = option_codes::END;

        let result = parser.parse(&packet);
        assert!(result.is_ok());

        let dhcp = result.unwrap();
        assert_eq!(dhcp.op, 1);
        assert_eq!(dhcp.xid, 0x12345678);
        assert_eq!(
            dhcp.chaddr,
            MacAddr6::new(0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff)
        );
        assert_eq!(dhcp.message_type(), Some(DhcpMessageType::Discover));
    }

    #[test]
    fn test_packet_too_short() {
        let parser = DhcpParser::new();
        let packet = vec![0u8; 100];

        let result = parser.parse(&packet);
        assert!(matches!(result, Err(ParseError::PacketTooShort { .. })));
    }
}
