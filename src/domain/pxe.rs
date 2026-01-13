//! PXE-specific domain models.

use std::fmt;

/// PXE client system architecture types as defined in RFC 4578.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PxeClientArch {
    IntelX86Bios,
    NecPc98,
    Efi386,
    EfiBC,
    EfiX64,
    EfiArm32,
    EfiArm64,
    Unknown(u16),
}

impl PxeClientArch {
    pub fn from_u16(value: u16) -> Self {
        match value {
            0 => Self::IntelX86Bios,
            1 => Self::NecPc98,
            2 => Self::Efi386,
            6 => Self::EfiBC,
            7 => Self::EfiX64,
            9 => Self::EfiArm32,
            11 => Self::EfiArm64,
            other => Self::Unknown(other),
        }
    }

    pub fn is_efi(&self) -> bool {
        matches!(
            self,
            Self::Efi386 | Self::EfiBC | Self::EfiX64 | Self::EfiArm32 | Self::EfiArm64
        )
    }

    pub fn is_bios(&self) -> bool {
        matches!(self, Self::IntelX86Bios)
    }
}

impl fmt::Display for PxeClientArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntelX86Bios => write!(f, "x86 BIOS"),
            Self::NecPc98 => write!(f, "NEC/PC98"),
            Self::Efi386 => write!(f, "EFI x86"),
            Self::EfiBC => write!(f, "EFI BC"),
            Self::EfiX64 => write!(f, "EFI x64"),
            Self::EfiArm32 => write!(f, "EFI ARM32"),
            Self::EfiArm64 => write!(f, "EFI ARM64"),
            Self::Unknown(code) => write!(f, "Unknown({code})"),
        }
    }
}

/// Parsed PXE information from a DHCP packet.
#[derive(Debug, Clone)]
pub struct PxeInfo {
    /// The vendor class identifier string (e.g., "PXEClient:Arch:00000:UNDI:002001")
    pub vendor_class: String,
    /// Parsed client architecture
    pub architecture: Option<PxeClientArch>,
    /// Client UUID if present
    pub uuid: Option<String>,
}

impl PxeInfo {
    /// Parse PXE info from vendor class identifier string.
    pub fn from_vendor_class(vendor_class: &str) -> Option<Self> {
        if !vendor_class.starts_with("PXEClient") {
            return None;
        }

        // Parse architecture from vendor class string if present
        // Format: PXEClient:Arch:XXXXX:UNDI:YYYYYY
        let architecture = Self::parse_arch_from_vendor_class(vendor_class);

        Some(Self {
            vendor_class: vendor_class.to_string(),
            architecture,
            uuid: None,
        })
    }

    fn parse_arch_from_vendor_class(vendor_class: &str) -> Option<PxeClientArch> {
        // Look for "Arch:XXXXX" pattern
        let arch_prefix = "Arch:";
        let arch_start = vendor_class.find(arch_prefix)?;
        let arch_value_start = arch_start + arch_prefix.len();

        // Extract the numeric part
        let remaining = &vendor_class[arch_value_start..];
        let arch_end = remaining
            .find(':')
            .unwrap_or(remaining.len())
            .min(remaining.find(' ').unwrap_or(remaining.len()));

        let arch_str = &remaining[..arch_end];
        let arch_value: u16 = arch_str.parse().ok()?;

        Some(PxeClientArch::from_u16(arch_value))
    }

    /// Set the architecture from Option 93 if available.
    pub fn with_architecture(mut self, arch: u16) -> Self {
        self.architecture = Some(PxeClientArch::from_u16(arch));
        self
    }

    /// Set the UUID if available.
    pub fn with_uuid(mut self, uuid: &[u8]) -> Self {
        if uuid.len() >= 17 && uuid[0] == 0 {
            // Type 0 = UUID/GUID, skip the type byte
            self.uuid = Some(format_uuid(&uuid[1..17]));
        } else if uuid.len() >= 16 {
            self.uuid = Some(format_uuid(&uuid[..16]));
        }
        self
    }
}

/// Format a 16-byte UUID as a string.
fn format_uuid(bytes: &[u8]) -> String {
    if bytes.len() < 16 {
        return hex::encode(bytes);
    }

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

// Simple hex encoding since we don't want to add another dependency
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod pxe_client_arch_tests {
        use super::*;

        #[test]
        fn test_from_u16_known_values() {
            assert_eq!(PxeClientArch::from_u16(0), PxeClientArch::IntelX86Bios);
            assert_eq!(PxeClientArch::from_u16(1), PxeClientArch::NecPc98);
            assert_eq!(PxeClientArch::from_u16(2), PxeClientArch::Efi386);
            assert_eq!(PxeClientArch::from_u16(6), PxeClientArch::EfiBC);
            assert_eq!(PxeClientArch::from_u16(7), PxeClientArch::EfiX64);
            assert_eq!(PxeClientArch::from_u16(9), PxeClientArch::EfiArm32);
            assert_eq!(PxeClientArch::from_u16(11), PxeClientArch::EfiArm64);
        }

        #[test]
        fn test_from_u16_unknown_values() {
            assert_eq!(PxeClientArch::from_u16(3), PxeClientArch::Unknown(3));
            assert_eq!(PxeClientArch::from_u16(100), PxeClientArch::Unknown(100));
            assert_eq!(PxeClientArch::from_u16(65535), PxeClientArch::Unknown(65535));
        }

        #[test]
        fn test_is_efi() {
            assert!(PxeClientArch::Efi386.is_efi());
            assert!(PxeClientArch::EfiBC.is_efi());
            assert!(PxeClientArch::EfiX64.is_efi());
            assert!(PxeClientArch::EfiArm32.is_efi());
            assert!(PxeClientArch::EfiArm64.is_efi());

            assert!(!PxeClientArch::IntelX86Bios.is_efi());
            assert!(!PxeClientArch::NecPc98.is_efi());
            assert!(!PxeClientArch::Unknown(99).is_efi());
        }

        #[test]
        fn test_is_bios() {
            assert!(PxeClientArch::IntelX86Bios.is_bios());

            assert!(!PxeClientArch::Efi386.is_bios());
            assert!(!PxeClientArch::EfiX64.is_bios());
            assert!(!PxeClientArch::NecPc98.is_bios());
            assert!(!PxeClientArch::Unknown(0).is_bios());
        }

        #[test]
        fn test_display() {
            assert_eq!(format!("{}", PxeClientArch::IntelX86Bios), "x86 BIOS");
            assert_eq!(format!("{}", PxeClientArch::NecPc98), "NEC/PC98");
            assert_eq!(format!("{}", PxeClientArch::Efi386), "EFI x86");
            assert_eq!(format!("{}", PxeClientArch::EfiBC), "EFI BC");
            assert_eq!(format!("{}", PxeClientArch::EfiX64), "EFI x64");
            assert_eq!(format!("{}", PxeClientArch::EfiArm32), "EFI ARM32");
            assert_eq!(format!("{}", PxeClientArch::EfiArm64), "EFI ARM64");
            assert_eq!(format!("{}", PxeClientArch::Unknown(42)), "Unknown(42)");
        }
    }

    mod pxe_info_tests {
        use super::*;

        #[test]
        fn test_from_vendor_class_valid_pxe() {
            let info = PxeInfo::from_vendor_class("PXEClient:Arch:00007:UNDI:003016");
            assert!(info.is_some());

            let info = info.unwrap();
            assert_eq!(info.vendor_class, "PXEClient:Arch:00007:UNDI:003016");
            assert_eq!(info.architecture, Some(PxeClientArch::EfiX64));
            assert!(info.uuid.is_none());
        }

        #[test]
        fn test_from_vendor_class_bios() {
            let info = PxeInfo::from_vendor_class("PXEClient:Arch:00000:UNDI:002001").unwrap();
            assert_eq!(info.architecture, Some(PxeClientArch::IntelX86Bios));
        }

        #[test]
        fn test_from_vendor_class_minimal() {
            let info = PxeInfo::from_vendor_class("PXEClient");
            assert!(info.is_some());

            let info = info.unwrap();
            assert_eq!(info.vendor_class, "PXEClient");
            assert!(info.architecture.is_none());
        }

        #[test]
        fn test_from_vendor_class_non_pxe() {
            assert!(PxeInfo::from_vendor_class("MSFT 5.0").is_none());
            assert!(PxeInfo::from_vendor_class("dhcpcd").is_none());
            assert!(PxeInfo::from_vendor_class("").is_none());
            assert!(PxeInfo::from_vendor_class("pxeclient").is_none()); // Case sensitive
        }

        #[test]
        fn test_from_vendor_class_arch_without_colon() {
            let info = PxeInfo::from_vendor_class("PXEClient:Arch:00007").unwrap();
            assert_eq!(info.architecture, Some(PxeClientArch::EfiX64));
        }

        #[test]
        fn test_from_vendor_class_arch_with_space() {
            let info = PxeInfo::from_vendor_class("PXEClient:Arch:00007 extra").unwrap();
            assert_eq!(info.architecture, Some(PxeClientArch::EfiX64));
        }

        #[test]
        fn test_from_vendor_class_invalid_arch_number() {
            let info = PxeInfo::from_vendor_class("PXEClient:Arch:invalid:UNDI").unwrap();
            assert!(info.architecture.is_none());
        }

        #[test]
        fn test_with_architecture() {
            let info = PxeInfo::from_vendor_class("PXEClient")
                .unwrap()
                .with_architecture(7);

            assert_eq!(info.architecture, Some(PxeClientArch::EfiX64));
        }

        #[test]
        fn test_with_architecture_overrides_parsed() {
            let info = PxeInfo::from_vendor_class("PXEClient:Arch:00000:UNDI:002001")
                .unwrap()
                .with_architecture(7);

            // Option 93 should override the parsed value
            assert_eq!(info.architecture, Some(PxeClientArch::EfiX64));
        }

        #[test]
        fn test_with_uuid_type_0() {
            let uuid_bytes = [
                0x00, // Type 0
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f, 0x10,
            ];

            let info = PxeInfo::from_vendor_class("PXEClient")
                .unwrap()
                .with_uuid(&uuid_bytes);

            assert!(info.uuid.is_some());
            assert_eq!(
                info.uuid.unwrap(),
                "01020304-0506-0708-090a-0b0c0d0e0f10"
            );
        }

        #[test]
        fn test_with_uuid_raw_16_bytes() {
            let uuid_bytes = [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f, 0x10,
            ];

            let info = PxeInfo::from_vendor_class("PXEClient")
                .unwrap()
                .with_uuid(&uuid_bytes);

            assert!(info.uuid.is_some());
            assert_eq!(
                info.uuid.unwrap(),
                "01020304-0506-0708-090a-0b0c0d0e0f10"
            );
        }

        #[test]
        fn test_with_uuid_too_short() {
            let short_uuid = [0x01, 0x02, 0x03];

            let info = PxeInfo::from_vendor_class("PXEClient")
                .unwrap()
                .with_uuid(&short_uuid);

            // UUID should not be set if too short
            assert!(info.uuid.is_none());
        }

        #[test]
        fn test_with_uuid_empty() {
            let info = PxeInfo::from_vendor_class("PXEClient")
                .unwrap()
                .with_uuid(&[]);

            assert!(info.uuid.is_none());
        }
    }

    mod format_uuid_tests {
        use super::*;

        #[test]
        fn test_format_uuid_valid() {
            let bytes = [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
                0xdd, 0xee, 0xff,
            ];
            assert_eq!(
                format_uuid(&bytes),
                "00112233-4455-6677-8899-aabbccddeeff"
            );
        }

        #[test]
        fn test_format_uuid_short() {
            let bytes = [0x01, 0x02, 0x03];
            assert_eq!(format_uuid(&bytes), "010203");
        }

        #[test]
        fn test_format_uuid_empty() {
            assert_eq!(format_uuid(&[]), "");
        }
    }

    mod hex_tests {
        use super::hex;

        #[test]
        fn test_encode() {
            assert_eq!(hex::encode(&[0x00, 0xff, 0x0a, 0xbc]), "00ff0abc");
            assert_eq!(hex::encode(&[]), "");
            assert_eq!(hex::encode(&[0x42]), "42");
        }
    }
}
