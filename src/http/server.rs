//! HTTP server for serving cloud-init user-data, meta-data, and boot files.
//!
//! Serves NoCloud datasource files for Ubuntu autoinstall, and optionally
//! serves boot files (kernel, initrd) for faster transfers than TFTP.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, error, info, warn};

/// Cloud-init HTTP server for serving autoinstall data and boot files.
pub struct CloudInitServer {
    /// Directory containing user-data and meta-data files.
    data_dir: PathBuf,
    /// Optional directory for serving boot files (kernel, initrd).
    boot_dir: Option<PathBuf>,
    /// Optional directory for serving ISO files.
    iso_dir: Option<PathBuf>,
    /// Bind address for HTTP server.
    bind_addr: SocketAddr,
    /// Running flag.
    running: Arc<AtomicBool>,
    /// User-data content (can be template or static).
    user_data: Option<String>,
    /// Meta-data content.
    meta_data: Option<String>,
}

impl CloudInitServer {
    /// Create a new cloud-init HTTP server.
    pub fn new<P: AsRef<Path>>(data_dir: P, bind_addr: SocketAddr) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            boot_dir: None,
            iso_dir: None,
            bind_addr,
            running: Arc::new(AtomicBool::new(false)),
            user_data: None,
            meta_data: None,
        }
    }

    /// Set directory for serving boot files (kernel, initrd).
    pub fn with_boot_dir<P: AsRef<Path>>(mut self, boot_dir: P) -> Self {
        self.boot_dir = Some(boot_dir.as_ref().to_path_buf());
        self
    }

    /// Set directory for serving ISO files.
    pub fn with_iso_dir<P: AsRef<Path>>(mut self, iso_dir: P) -> Self {
        self.iso_dir = Some(iso_dir.as_ref().to_path_buf());
        self
    }

    /// Set user-data content directly.
    pub fn with_user_data(mut self, content: String) -> Self {
        self.user_data = Some(content);
        self
    }

    /// Set meta-data content directly.
    pub fn with_meta_data(mut self, content: String) -> Self {
        self.meta_data = Some(content);
        self
    }

    /// Load user-data from a file.
    pub fn load_user_data<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read user-data from {:?}", path.as_ref()))?;
        self.user_data = Some(content);
        Ok(())
    }

    /// Load meta-data from a file.
    pub fn load_meta_data<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read meta-data from {:?}", path.as_ref()))?;
        self.meta_data = Some(content);
        Ok(())
    }

    /// Get the running flag for external control.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.running)
    }

    /// Get the server URL for use in boot parameters.
    pub fn url(&self) -> String {
        format!("http://{}/", self.bind_addr)
    }

    /// Run the HTTP server.
    pub fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.bind_addr)
            .with_context(|| format!("Failed to bind HTTP server to {}", self.bind_addr))?;

        listener
            .set_nonblocking(true)
            .context("Failed to set non-blocking")?;

        self.running.store(true, Ordering::SeqCst);
        info!("Cloud-init HTTP server listening on {}", self.bind_addr);

        while self.running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, addr)) => {
                    debug!("HTTP connection from {}", addr);
                    if let Err(e) = self.handle_connection(stream, addr) {
                        warn!("Error handling HTTP request from {}: {}", addr, e);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                }
            }
        }

        info!("Cloud-init HTTP server stopped");
        Ok(())
    }

    /// Handle an incoming HTTP connection.
    fn handle_connection(&self, mut stream: TcpStream, addr: SocketAddr) -> Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?; // Longer timeout for large files

        let mut buffer = [0u8; 4096];
        let bytes_read = stream.read(&mut buffer)?;

        if bytes_read == 0 {
            return Ok(());
        }

        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        let (method, path) = self.parse_request(&request);

        info!("HTTP {} {} from {}", method, path, addr);

        // Handle ISO file requests (under /iso/ prefix)
        if method == "GET" && path.starts_with("/iso/") && self.iso_dir.is_some() {
            let iso_path = path.strip_prefix("/iso/").unwrap_or("");
            if let Some(served) = self.try_serve_iso_file(iso_path, &mut stream) {
                if served {
                    return Ok(());
                }
                // File not found, fall through to 404
            }
        }

        // Handle boot file requests separately (binary data)
        if method == "GET" && self.boot_dir.is_some() {
            if let Some(response) = self.try_serve_boot_file(&path, &mut stream) {
                if !response {
                    // File not found or error, fall through to text responses
                } else {
                    return Ok(());
                }
            }
        }

        let response = match (method.as_str(), path.as_str()) {
            ("GET", "/user-data") | ("GET", "/user-data/") => {
                self.serve_user_data()
            }
            ("GET", "/meta-data") | ("GET", "/meta-data/") => {
                self.serve_meta_data()
            }
            ("GET", "/vendor-data") | ("GET", "/vendor-data/") => {
                self.serve_vendor_data()
            }
            ("GET", "/") => {
                self.serve_index()
            }
            _ => {
                self.serve_not_found(&path)
            }
        };

        stream.write_all(response.as_bytes())?;
        stream.flush()?;

        Ok(())
    }

    /// Parse HTTP request line.
    fn parse_request(&self, request: &str) -> (String, String) {
        let first_line = request.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();

        if parts.len() >= 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            ("GET".to_string(), "/".to_string())
        }
    }

    /// Serve user-data content.
    fn serve_user_data(&self) -> String {
        let content = self.user_data.clone().unwrap_or_else(|| {
            // Try to load from file
            let path = self.data_dir.join("user-data");
            fs::read_to_string(&path).unwrap_or_else(|_| self.default_user_data())
        });

        self.http_response(200, "text/yaml", &content)
    }

    /// Serve meta-data content.
    fn serve_meta_data(&self) -> String {
        let content = self.meta_data.clone().unwrap_or_else(|| {
            // Try to load from file
            let path = self.data_dir.join("meta-data");
            fs::read_to_string(&path).unwrap_or_else(|_| self.default_meta_data())
        });

        self.http_response(200, "text/yaml", &content)
    }

    /// Serve vendor-data (usually empty).
    fn serve_vendor_data(&self) -> String {
        let path = self.data_dir.join("vendor-data");
        let content = fs::read_to_string(&path).unwrap_or_default();
        self.http_response(200, "text/yaml", &content)
    }

    /// Serve index listing available endpoints.
    fn serve_index(&self) -> String {
        let content = "user-data\nmeta-data\nvendor-data\n";
        self.http_response(200, "text/plain", content)
    }

    /// Serve 404 Not Found.
    fn serve_not_found(&self, path: &str) -> String {
        let content = format!("Not Found: {}\n", path);
        self.http_response(404, "text/plain", &content)
    }

    /// Try to serve a boot file directly to the stream.
    /// Returns Some(true) if file was served, Some(false) if not found, None if boot_dir not set.
    fn try_serve_boot_file(&self, path: &str, stream: &mut TcpStream) -> Option<bool> {
        let boot_dir = self.boot_dir.as_ref()?;

        // Sanitize path - prevent directory traversal
        let clean_path = path.trim_start_matches('/');
        if clean_path.is_empty() || clean_path.contains("..") {
            return Some(false);
        }

        let file_path = boot_dir.join(clean_path);

        // Check if file exists and is within boot_dir
        if !file_path.starts_with(boot_dir) || !file_path.is_file() {
            return Some(false);
        }

        match File::open(&file_path) {
            Ok(mut file) => {
                // Get file size for Content-Length
                let metadata = match file.metadata() {
                    Ok(m) => m,
                    Err(_) => return Some(false),
                };
                let file_size = metadata.len();

                info!("HTTP: Serving boot file {} ({} bytes)", clean_path, file_size);

                // Determine content type
                let content_type = if clean_path.ends_with(".efi") {
                    "application/efi"
                } else if clean_path.ends_with(".cfg") || clean_path.ends_with(".conf") {
                    "text/plain"
                } else {
                    "application/octet-stream"
                };

                // Build and send response header
                let header = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: {}\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\
                     \r\n",
                    content_type,
                    file_size
                );

                if let Err(e) = stream.write_all(header.as_bytes()) {
                    error!("Failed to write HTTP header: {}", e);
                    return Some(true); // We tried, connection is broken
                }

                // Stream file content in chunks
                let mut buffer = [0u8; 65536]; // 64KB chunks
                let mut total_sent = 0u64;
                loop {
                    match file.read(&mut buffer) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            if let Err(e) = stream.write_all(&buffer[..n]) {
                                error!("Failed to write boot file data: {}", e);
                                return Some(true);
                            }
                            total_sent += n as u64;
                        }
                        Err(e) => {
                            error!("Failed to read boot file {}: {}", clean_path, e);
                            return Some(true);
                        }
                    }
                }

                if let Err(e) = stream.flush() {
                    error!("Failed to flush stream: {}", e);
                }

                info!("HTTP: Transfer complete: {} ({} bytes)", clean_path, total_sent);
                Some(true)
            }
            Err(_) => Some(false),
        }
    }

    /// Try to serve an ISO file directly to the stream.
    /// Returns Some(true) if file was served, Some(false) if not found, None if iso_dir not set.
    fn try_serve_iso_file(&self, path: &str, stream: &mut TcpStream) -> Option<bool> {
        let iso_dir = self.iso_dir.as_ref()?;

        // Sanitize path - prevent directory traversal
        let clean_path = path.trim_start_matches('/');
        if clean_path.is_empty() || clean_path.contains("..") {
            return Some(false);
        }

        let file_path = iso_dir.join(clean_path);

        // Check if file exists and is within iso_dir
        if !file_path.starts_with(iso_dir) || !file_path.is_file() {
            return Some(false);
        }

        match File::open(&file_path) {
            Ok(mut file) => {
                let metadata = match file.metadata() {
                    Ok(m) => m,
                    Err(_) => return Some(false),
                };
                let file_size = metadata.len();

                info!("HTTP: Serving ISO file {} ({:.2} GB)",
                    clean_path,
                    file_size as f64 / 1_073_741_824.0
                );

                // Build and send response header
                let header = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: application/x-iso9660-image\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\
                     \r\n",
                    file_size
                );

                if let Err(e) = stream.write_all(header.as_bytes()) {
                    error!("Failed to write HTTP header: {}", e);
                    return Some(true);
                }

                // Stream file content in larger chunks for ISO files
                let mut buffer = [0u8; 262144]; // 256KB chunks for ISOs
                let mut total_sent = 0u64;
                let mut last_progress = 0u64;
                let progress_interval = 100 * 1024 * 1024; // Log every 100MB

                loop {
                    match file.read(&mut buffer) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            if let Err(e) = stream.write_all(&buffer[..n]) {
                                error!("Failed to write ISO data: {}", e);
                                return Some(true);
                            }
                            total_sent += n as u64;

                            // Log progress for large files
                            if total_sent - last_progress >= progress_interval {
                                let percent = (total_sent as f64 / file_size as f64) * 100.0;
                                info!("HTTP: ISO transfer progress: {:.1}% ({:.0} MB / {:.0} MB)",
                                    percent,
                                    total_sent as f64 / 1_048_576.0,
                                    file_size as f64 / 1_048_576.0
                                );
                                last_progress = total_sent;
                            }
                        }
                        Err(e) => {
                            error!("Failed to read ISO file {}: {}", clean_path, e);
                            return Some(true);
                        }
                    }
                }

                if let Err(e) = stream.flush() {
                    error!("Failed to flush stream: {}", e);
                }

                info!("HTTP: ISO transfer complete: {} ({:.2} GB)",
                    clean_path,
                    total_sent as f64 / 1_073_741_824.0
                );
                Some(true)
            }
            Err(_) => Some(false),
        }
    }

    /// Build HTTP response.
    fn http_response(&self, status: u16, content_type: &str, body: &str) -> String {
        let status_text = match status {
            200 => "OK",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Unknown",
        };

        format!(
            "HTTP/1.1 {} {}\r\n\
             Content-Type: {}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            status,
            status_text,
            content_type,
            body.len(),
            body
        )
    }

    /// Default user-data for Ubuntu autoinstall.
    fn default_user_data(&self) -> String {
        r#"#cloud-config
autoinstall:
  version: 1
  locale: en_US.UTF-8
  keyboard:
    layout: us
  identity:
    hostname: ubuntu-server
    username: ubuntu
    password: "$6$rounds=4096$xyz$hashed"
  ssh:
    install-server: true
    allow-pw: true
  storage:
    layout:
      name: lvm
"#.to_string()
    }

    /// Default meta-data.
    fn default_meta_data(&self) -> String {
        "instance-id: iid-local01\nlocal-hostname: ubuntu-server\n".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_new() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp/cloud-init", addr);
        assert_eq!(server.data_dir, PathBuf::from("/tmp/cloud-init"));
        assert_eq!(server.bind_addr, addr);
    }

    #[test]
    fn test_url() {
        let addr = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 100), 8080));
        let server = CloudInitServer::new("/tmp", addr);
        assert_eq!(server.url(), "http://192.168.1.100:8080/");
    }

    #[test]
    fn test_with_user_data() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr)
            .with_user_data("test-data".to_string());
        assert_eq!(server.user_data, Some("test-data".to_string()));
    }

    #[test]
    fn test_with_meta_data() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr)
            .with_meta_data("instance-id: test".to_string());
        assert_eq!(server.meta_data, Some("instance-id: test".to_string()));
    }

    #[test]
    fn test_running_flag() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);
        let flag = server.running_flag();
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_parse_request() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let (method, path) = server.parse_request("GET /user-data HTTP/1.1\r\nHost: test\r\n");
        assert_eq!(method, "GET");
        assert_eq!(path, "/user-data");
    }

    #[test]
    fn test_parse_request_empty() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let (method, path) = server.parse_request("");
        assert_eq!(method, "GET");
        assert_eq!(path, "/");
    }

    #[test]
    fn test_http_response() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.http_response(200, "text/plain", "hello");
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Length: 5"));
        assert!(response.contains("hello"));
    }

    #[test]
    fn test_default_user_data() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let data = server.default_user_data();
        assert!(data.contains("#cloud-config"));
        assert!(data.contains("autoinstall:"));
    }

    #[test]
    fn test_default_meta_data() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let data = server.default_meta_data();
        assert!(data.contains("instance-id:"));
    }

    #[test]
    fn test_serve_index() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.serve_index();
        assert!(response.contains("user-data"));
        assert!(response.contains("meta-data"));
    }

    #[test]
    fn test_serve_not_found() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.serve_not_found("/unknown");
        assert!(response.contains("404"));
        assert!(response.contains("Not Found: /unknown"));
    }

    #[test]
    fn test_serve_user_data_with_custom() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr)
            .with_user_data("custom-user-data".to_string());

        let response = server.serve_user_data();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("custom-user-data"));
    }

    #[test]
    fn test_serve_user_data_default() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/nonexistent/path", addr);

        let response = server.serve_user_data();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("#cloud-config"));
    }

    #[test]
    fn test_serve_meta_data_with_custom() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr)
            .with_meta_data("instance-id: custom-id".to_string());

        let response = server.serve_meta_data();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("instance-id: custom-id"));
    }

    #[test]
    fn test_serve_meta_data_default() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/nonexistent/path", addr);

        let response = server.serve_meta_data();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("instance-id:"));
    }

    #[test]
    fn test_serve_vendor_data() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/nonexistent/path", addr);

        let response = server.serve_vendor_data();
        assert!(response.contains("HTTP/1.1 200 OK"));
    }

    #[test]
    fn test_http_response_404() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.http_response(404, "text/plain", "not found");
        assert!(response.contains("HTTP/1.1 404 Not Found"));
        assert!(response.contains("not found"));
    }

    #[test]
    fn test_http_response_500() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.http_response(500, "text/plain", "error");
        assert!(response.contains("HTTP/1.1 500 Internal Server Error"));
    }

    #[test]
    fn test_http_response_unknown_status() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.http_response(418, "text/plain", "teapot");
        assert!(response.contains("HTTP/1.1 418 Unknown"));
    }

    #[test]
    fn test_parse_request_post() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let (method, path) = server.parse_request("POST /api HTTP/1.1\r\n");
        assert_eq!(method, "POST");
        assert_eq!(path, "/api");
    }

    #[test]
    fn test_parse_request_with_query() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let (method, path) = server.parse_request("GET /user-data?mac=aa:bb HTTP/1.1\r\n");
        assert_eq!(method, "GET");
        assert_eq!(path, "/user-data?mac=aa:bb");
    }

    #[test]
    fn test_url_different_port() {
        let addr = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 3000));
        let server = CloudInitServer::new("/tmp", addr);
        assert_eq!(server.url(), "http://10.0.0.1:3000/");
    }

    #[test]
    fn test_running_flag_can_be_set() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);
        let flag = server.running_flag();
        flag.store(true, Ordering::SeqCst);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_serve_index_content_type() {
        let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), 8080));
        let server = CloudInitServer::new("/tmp", addr);

        let response = server.serve_index();
        assert!(response.contains("Content-Type: text/plain"));
        assert!(response.contains("vendor-data"));
    }
}
