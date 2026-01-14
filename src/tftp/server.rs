//! TFTP server implementation.
//!
//! A simple TFTP server for serving PXE boot files.
//! Implements RFC 1350 (TFTP) with RFC 2347 (options) support.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tracing::{debug, error, info, warn};

/// TFTP opcodes
const OPCODE_RRQ: u16 = 1;   // Read request
const OPCODE_WRQ: u16 = 2;   // Write request (not supported)
const OPCODE_DATA: u16 = 3;  // Data packet
const OPCODE_ACK: u16 = 4;   // Acknowledgment
const OPCODE_ERROR: u16 = 5; // Error
const OPCODE_OACK: u16 = 6;  // Option acknowledgment (RFC 2347)

/// TFTP error codes
const _ERROR_NOT_DEFINED: u16 = 0;
const ERROR_FILE_NOT_FOUND: u16 = 1;
const ERROR_ACCESS_VIOLATION: u16 = 2;
const _ERROR_ILLEGAL_OPERATION: u16 = 4;

/// Default block size
const DEFAULT_BLOCK_SIZE: usize = 512;

/// Maximum block size (RFC 2348)
const MAX_BLOCK_SIZE: usize = 65464;

/// TFTP server for serving boot files.
pub struct TftpServer {
    /// Root directory for TFTP files.
    root: PathBuf,
    /// Bind address.
    bind_addr: SocketAddr,
    /// Running flag.
    running: Arc<AtomicBool>,
}

impl TftpServer {
    /// Create a new TFTP server.
    ///
    /// # Arguments
    /// * `root` - Root directory to serve files from
    /// * `bind_addr` - Address to bind to (default: 0.0.0.0:69)
    pub fn new(root: impl AsRef<Path>, bind_addr: SocketAddr) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            bind_addr,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a handle to stop the server.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Start the TFTP server.
    ///
    /// This runs in a loop until `running` is set to false.
    pub fn run(&self) -> Result<()> {
        let socket = UdpSocket::bind(self.bind_addr)
            .with_context(|| format!("Failed to bind TFTP socket to {}", self.bind_addr))?;

        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .context("Failed to set socket timeout")?;

        info!("TFTP server listening on {}", self.bind_addr);
        info!("Serving files from: {}", self.root.display());

        self.running.store(true, Ordering::SeqCst);

        let mut buf = [0u8; 65536];

        while self.running.load(Ordering::SeqCst) {
            match socket.recv_from(&mut buf) {
                Ok((len, client_addr)) => {
                    if len < 4 {
                        continue;
                    }

                    let opcode = u16::from_be_bytes([buf[0], buf[1]]);

                    match opcode {
                        OPCODE_RRQ => {
                            let request = &buf[2..len];
                            self.handle_read_request(request, client_addr);
                        }
                        OPCODE_WRQ => {
                            warn!("Write request from {} denied (read-only server)", client_addr);
                            self.send_error(&socket, client_addr, ERROR_ACCESS_VIOLATION, "Write not supported");
                        }
                        _ => {
                            debug!("Unknown opcode {} from {}", opcode, client_addr);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Timeout, check running flag
                    continue;
                }
                Err(e) => {
                    error!("TFTP receive error: {}", e);
                }
            }
        }

        info!("TFTP server stopped");
        Ok(())
    }

    /// Handle a read request in a separate thread.
    fn handle_read_request(&self, request: &[u8], client_addr: SocketAddr) {
        // Parse filename and mode
        let parts: Vec<&[u8]> = request.split(|&b| b == 0).collect();
        if parts.is_empty() {
            return;
        }

        let filename = match std::str::from_utf8(parts[0]) {
            Ok(f) => f.to_string(),
            Err(_) => return,
        };

        let mode = if parts.len() > 1 {
            std::str::from_utf8(parts[1]).unwrap_or("octet").to_lowercase()
        } else {
            "octet".to_string()
        };

        // Parse options (RFC 2347)
        let mut options: HashMap<String, String> = HashMap::new();
        let mut i = 2;
        while i + 1 < parts.len() {
            if let (Ok(name), Ok(value)) = (
                std::str::from_utf8(parts[i]),
                std::str::from_utf8(parts[i + 1]),
            ) {
                if !name.is_empty() && !value.is_empty() {
                    options.insert(name.to_lowercase(), value.to_string());
                }
            }
            i += 2;
        }

        let root = self.root.clone();

        // Spawn a thread to handle the transfer
        thread::spawn(move || {
            if let Err(e) = Self::handle_transfer(root, filename, mode, options, client_addr) {
                error!("TFTP transfer error for {}: {}", client_addr, e);
            }
        });
    }

    /// Handle a file transfer.
    fn handle_transfer(
        root: PathBuf,
        filename: String,
        _mode: String,
        options: HashMap<String, String>,
        client_addr: SocketAddr,
    ) -> Result<()> {
        // Sanitize and resolve the file path
        let clean_filename = filename.trim_start_matches('/').replace("..", "");
        let file_path = root.join(&clean_filename);

        // Security check: ensure we're still under root
        let canonical = file_path.canonicalize();
        let root_canonical = root.canonicalize().unwrap_or_else(|_| root.clone());

        let file_path = match canonical {
            Ok(path) if path.starts_with(&root_canonical) => path,
            Ok(path) => {
                warn!("TFTP: Access denied (outside root): {} -> {}", filename, path.display());
                let socket = UdpSocket::bind("0.0.0.0:0")?;
                Self::send_error_static(&socket, client_addr, ERROR_FILE_NOT_FOUND, "File not found");
                return Ok(());
            }
            Err(_) => {
                warn!("TFTP: File not found: {} (looked in {})", filename, root.join(&clean_filename).display());
                let socket = UdpSocket::bind("0.0.0.0:0")?;
                Self::send_error_static(&socket, client_addr, ERROR_FILE_NOT_FOUND, "File not found");
                return Ok(());
            }
        };

        info!("TFTP: {} requesting {}", client_addr, clean_filename);

        // Open the file
        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                warn!("TFTP: Cannot open {}: {}", file_path.display(), e);
                let socket = UdpSocket::bind("0.0.0.0:0")?;
                Self::send_error_static(&socket, client_addr, ERROR_FILE_NOT_FOUND, "File not found");
                return Ok(());
            }
        };

        // Get file size
        let file_size = file.metadata()?.len();

        // Determine block size
        let mut block_size = DEFAULT_BLOCK_SIZE;
        let mut tsize_requested = false;

        if let Some(blksize_str) = options.get("blksize") {
            if let Ok(requested) = blksize_str.parse::<usize>() {
                block_size = requested.min(MAX_BLOCK_SIZE).max(8);
            }
        }

        if options.contains_key("tsize") {
            tsize_requested = true;
        }

        // Create transfer socket (use ephemeral port)
        let socket = UdpSocket::bind("0.0.0.0:0")
            .context("Failed to bind transfer socket")?;

        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;

        // Send OACK if options were requested
        if !options.is_empty() {
            let mut oack = vec![0u8, OPCODE_OACK as u8];

            if block_size != DEFAULT_BLOCK_SIZE {
                oack.extend_from_slice(b"blksize\0");
                oack.extend_from_slice(block_size.to_string().as_bytes());
                oack.push(0);
            }

            if tsize_requested {
                oack.extend_from_slice(b"tsize\0");
                oack.extend_from_slice(file_size.to_string().as_bytes());
                oack.push(0);
            }

            socket.send_to(&oack, client_addr)?;

            // Wait for ACK of OACK (block 0)
            let mut ack_buf = [0u8; 4];
            match socket.recv_from(&mut ack_buf) {
                Ok((4, _)) => {
                    let opcode = u16::from_be_bytes([ack_buf[0], ack_buf[1]]);
                    let block = u16::from_be_bytes([ack_buf[2], ack_buf[3]]);
                    if opcode != OPCODE_ACK || block != 0 {
                        return Err(anyhow!("Expected ACK for OACK"));
                    }
                }
                Ok(_) => return Err(anyhow!("Invalid ACK for OACK")),
                Err(e) => return Err(anyhow!("Timeout waiting for OACK ACK: {}", e)),
            }
        }

        // Transfer the file
        let mut block_num: u16 = 1;
        let mut buf = vec![0u8; block_size];
        let mut total_sent = 0u64;

        loop {
            // Read a block
            let bytes_read = file.read(&mut buf)?;

            // Build DATA packet
            let mut data_packet = Vec::with_capacity(4 + bytes_read);
            data_packet.extend_from_slice(&OPCODE_DATA.to_be_bytes());
            data_packet.extend_from_slice(&block_num.to_be_bytes());
            data_packet.extend_from_slice(&buf[..bytes_read]);

            // Send with retries
            let mut retries = 0;
            loop {
                socket.send_to(&data_packet, client_addr)?;

                // Wait for ACK
                let mut ack_buf = [0u8; 4];
                match socket.recv_from(&mut ack_buf) {
                    Ok((len, _)) if len >= 4 => {
                        let opcode = u16::from_be_bytes([ack_buf[0], ack_buf[1]]);
                        let acked_block = u16::from_be_bytes([ack_buf[2], ack_buf[3]]);

                        if opcode == OPCODE_ACK && acked_block == block_num {
                            break; // Success
                        } else if opcode == OPCODE_ERROR {
                            return Err(anyhow!("Client sent error"));
                        }
                        // Duplicate ACK or wrong block, resend
                    }
                    Ok(_) => {
                        // Short packet, ignore and wait for proper ACK
                        retries += 1;
                        if retries > 5 {
                            return Err(anyhow!("Transfer timeout after 5 retries"));
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        retries += 1;
                        if retries > 5 {
                            return Err(anyhow!("Transfer timeout after 5 retries"));
                        }
                        debug!("TFTP: Retry {} for block {}", retries, block_num);
                    }
                    Err(e) => return Err(anyhow!("ACK receive error: {}", e)),
                }
            }

            total_sent += bytes_read as u64;

            // Check if this was the last block
            if bytes_read < block_size {
                info!(
                    "TFTP: Transfer complete: {} ({} bytes)",
                    clean_filename, total_sent
                );
                break;
            }

            block_num = block_num.wrapping_add(1);
        }

        Ok(())
    }

    /// Send an error packet.
    fn send_error(&self, socket: &UdpSocket, addr: SocketAddr, code: u16, message: &str) {
        Self::send_error_static(socket, addr, code, message);
    }

    /// Send an error packet (static version).
    fn send_error_static(socket: &UdpSocket, addr: SocketAddr, code: u16, message: &str) {
        let mut packet = Vec::with_capacity(5 + message.len());
        packet.extend_from_slice(&OPCODE_ERROR.to_be_bytes());
        packet.extend_from_slice(&code.to_be_bytes());
        packet.extend_from_slice(message.as_bytes());
        packet.push(0);

        let _ = socket.send_to(&packet, addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn test_new() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 69));
        let server = TftpServer::new("/tmp/tftp", addr);
        assert_eq!(server.root, PathBuf::from("/tmp/tftp"));
        assert_eq!(server.bind_addr, addr);
    }

    #[test]
    fn test_running_flag() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 69));
        let server = TftpServer::new("/tmp/tftp", addr);
        let flag = server.running_flag();
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_running_flag_can_be_set() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 69));
        let server = TftpServer::new("/tmp/tftp", addr);
        let flag = server.running_flag();
        flag.store(true, Ordering::SeqCst);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_new_with_different_port() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 6969));
        let server = TftpServer::new("/var/lib/tftp", addr);
        assert_eq!(server.bind_addr, addr);
    }

    #[test]
    fn test_new_with_specific_interface() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 69));
        let server = TftpServer::new("/srv/tftp", addr);
        assert_eq!(server.bind_addr, addr);
        assert_eq!(server.root, PathBuf::from("/srv/tftp"));
    }

    #[test]
    fn test_default_block_size_constant() {
        assert_eq!(DEFAULT_BLOCK_SIZE, 512);
    }

    #[test]
    fn test_max_block_size_constant() {
        assert_eq!(MAX_BLOCK_SIZE, 65464);
    }

    #[test]
    fn test_opcode_constants() {
        assert_eq!(OPCODE_RRQ, 1);
        assert_eq!(OPCODE_WRQ, 2);
        assert_eq!(OPCODE_DATA, 3);
        assert_eq!(OPCODE_ACK, 4);
        assert_eq!(OPCODE_ERROR, 5);
        assert_eq!(OPCODE_OACK, 6);
    }

    #[test]
    fn test_error_code_constants() {
        assert_eq!(ERROR_FILE_NOT_FOUND, 1);
        assert_eq!(ERROR_ACCESS_VIOLATION, 2);
    }
}
