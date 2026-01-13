//! PXE detection module.
//!
//! This module is responsible for detecting PXE boot requests
//! from parsed DHCP packets (SRP).

mod pxe_detector;

pub use pxe_detector::PxeDetector;
