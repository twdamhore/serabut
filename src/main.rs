//! Serabut - DHCP PXE Boot Server
//!
//! A PXE boot server that:
//! 1. Downloads and verifies Ubuntu netboot images
//! 2. Serves boot files via TFTP
//! 3. Provides PXE boot info via proxyDHCP
//! 4. Monitors and logs all PXE boot activity

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use clap::Parser;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use serabut::capture::PnetCapture;
use serabut::netboot::NetbootManager;
use serabut::proxydhcp::ProxyDhcpServer;
use serabut::reporter::ConsoleReporter;
use serabut::tftp::TftpServer;
use serabut::PxeListener;

/// DHCP PXE Boot Server
#[derive(Parser, Debug)]
#[command(author, version, about = "PXE boot server for Ubuntu netboot", long_about = None)]
struct Args {
    /// Network interface to listen on (required for server mode)
    #[arg(short, long)]
    interface: Option<String>,

    /// Directory to store netboot files (default: /var/lib/serabut)
    #[arg(long, default_value = "/var/lib/serabut")]
    data_dir: PathBuf,

    /// TFTP server port (default: 69)
    #[arg(long, default_value = "69")]
    tftp_port: u16,

    /// Skip netboot download (use existing files)
    #[arg(long)]
    skip_download: bool,

    /// Monitor only mode (no TFTP/proxyDHCP servers)
    #[arg(long)]
    monitor_only: bool,

    /// List available network interfaces and exit
    #[arg(long)]
    list_interfaces: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,
}

fn main() {
    // Initialize logging
    let log_level = if std::env::var("RUST_LOG").is_ok() {
        EnvFilter::from_default_env()
    } else {
        EnvFilter::new("serabut=info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    let args = Args::parse();

    // Handle --list-interfaces
    if args.list_interfaces {
        println!("Available network interfaces:\n");
        for iface in PnetCapture::list_interfaces() {
            println!("  {}", iface);
        }
        return;
    }

    // Global running flag for all threads
    let running = Arc::new(AtomicBool::new(true));

    // Set up Ctrl+C handler
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
            println!("\nReceived interrupt signal, shutting down...");
        })
        .expect("Error setting Ctrl-C handler");
    }

    // For server mode, require an interface and derive IP from it
    let server_ip = if !args.monitor_only {
        match &args.interface {
            Some(iface_name) => {
                match get_interface_ip(iface_name) {
                    Some(ip) => {
                        info!("Using interface {} with IP {}", iface_name, ip);
                        ip
                    }
                    None => {
                        eprintln!("Error: Could not get IP address for interface '{}'.", iface_name);
                        eprintln!("Make sure the interface exists and has an IPv4 address.");
                        eprintln!("\nUse --list-interfaces to see available interfaces.");
                        process::exit(1);
                    }
                }
            }
            None => {
                eprintln!("Error: Server mode requires an interface to be specified.");
                eprintln!("Use --interface <name> to specify the network interface.");
                eprintln!("Use --list-interfaces to see available interfaces.");
                eprintln!("\nOr use --monitor-only to just observe PXE traffic.");
                process::exit(1);
            }
        }
    } else {
        Ipv4Addr::UNSPECIFIED
    };

    // Step 1: Download/verify netboot image (unless skipped or monitor-only)
    let tftp_root = if !args.skip_download && !args.monitor_only {
        info!("=== Checking for latest Ubuntu netboot image ===");

        let manager = NetbootManager::new(&args.data_dir);
        match manager.ensure_netboot_ready() {
            Ok(root) => {
                info!("Netboot files ready at: {}", root.display());
                Some(root)
            }
            Err(e) => {
                error!("Failed to prepare netboot image: {}", e);
                if !args.monitor_only {
                    eprintln!("\nError: Could not prepare netboot image.");
                    eprintln!("Use --skip-download to use existing files.");
                    eprintln!("Use --monitor-only to just monitor PXE traffic.");
                    process::exit(1);
                }
                None
            }
        }
    } else if args.skip_download && !args.monitor_only {
        let tftp_root = args.data_dir.join("tftp");
        if !tftp_root.exists() {
            eprintln!("Error: TFTP root directory does not exist: {}", tftp_root.display());
            eprintln!("Run without --skip-download to download netboot files.");
            process::exit(1);
        }
        info!("Using existing netboot files at: {}", tftp_root.display());
        Some(tftp_root)
    } else {
        None
    };

    // Determine boot filenames based on what's available
    let (boot_file_bios, boot_file_efi) = if let Some(ref root) = tftp_root {
        detect_boot_files(root)
    } else {
        ("pxelinux.0".to_string(), "grubnetx64.efi.signed".to_string())
    };

    info!("BIOS boot file: {}", boot_file_bios);
    info!("EFI boot file: {}", boot_file_efi);

    // Step 2: Start TFTP server (unless monitor-only)
    let tftp_handle = if !args.monitor_only {
        if let Some(ref root) = tftp_root {
            let tftp_addr = SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::UNSPECIFIED,
                args.tftp_port,
            ));

            let tftp_server = TftpServer::new(root.clone(), tftp_addr);
            let tftp_running = tftp_server.running_flag();
            let global_running = running.clone();

            info!("=== Starting TFTP server ===");
            let handle = thread::spawn(move || {
                if let Err(e) = tftp_server.run() {
                    error!("TFTP server error: {}", e);
                }
            });

            // Link TFTP running to global running
            let tftp_running_clone = tftp_running.clone();
            thread::spawn(move || {
                while global_running.load(Ordering::SeqCst) {
                    thread::sleep(std::time::Duration::from_millis(100));
                }
                tftp_running_clone.store(false, Ordering::SeqCst);
            });

            Some(handle)
        } else {
            None
        }
    } else {
        None
    };

    // Step 3: Start proxyDHCP server (unless monitor-only)
    let proxydhcp_handle = if !args.monitor_only && server_ip != Ipv4Addr::UNSPECIFIED {
        let proxy_server = ProxyDhcpServer::new(
            server_ip,
            boot_file_bios.clone(),
            boot_file_efi.clone(),
        );
        let proxy_running = proxy_server.running_flag();
        let global_running = running.clone();

        info!("=== Starting proxyDHCP server ===");
        let handle = thread::spawn(move || {
            if let Err(e) = proxy_server.run() {
                error!("ProxyDHCP server error: {}", e);
            }
        });

        // Link proxyDHCP running to global running
        let proxy_running_clone = proxy_running.clone();
        thread::spawn(move || {
            while global_running.load(Ordering::SeqCst) {
                thread::sleep(std::time::Duration::from_millis(100));
            }
            proxy_running_clone.store(false, Ordering::SeqCst);
        });

        Some(handle)
    } else {
        if !args.monitor_only {
            warn!("ProxyDHCP server not started (no server IP)");
        }
        None
    };

    // Step 4: Start PXE monitor
    info!("=== Starting PXE boot monitor ===");

    // Create the capture backend
    let capture = match &args.interface {
        Some(name) => match PnetCapture::new(name) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: {}", e);
                eprintln!("\nUse --list-interfaces to see available interfaces.");
                process::exit(1);
            }
        },
        None => match PnetCapture::on_default_interface() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: {}", e);
                eprintln!("\nUse --interface <name> to specify an interface.");
                eprintln!("Use --list-interfaces to see available interfaces.");
                process::exit(1);
            }
        },
    };

    // Create the reporter
    let reporter = ConsoleReporter::new()
        .with_verbose(args.verbose)
        .with_colors(!args.no_color);

    // Create and run the listener
    let mut listener = PxeListener::new(capture, reporter);

    if let Err(e) = listener.run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }

    // Wait for other servers to stop
    if let Some(handle) = tftp_handle {
        let _ = handle.join();
    }
    if let Some(handle) = proxydhcp_handle {
        let _ = handle.join();
    }

    info!("Shutdown complete");
}

/// Get the IPv4 address for a specific network interface.
fn get_interface_ip(interface_name: &str) -> Option<Ipv4Addr> {
    use pnet::datalink;

    let interfaces = datalink::interfaces();

    interfaces
        .iter()
        .find(|iface| iface.name == interface_name)
        .and_then(|iface| {
            iface
                .ips
                .iter()
                .find_map(|ip| {
                    if let std::net::IpAddr::V4(v4) = ip.ip() {
                        if !v4.is_loopback() {
                            return Some(v4);
                        }
                    }
                    None
                })
        })
}

/// Detect available boot files in TFTP root.
fn detect_boot_files(root: &std::path::Path) -> (String, String) {
    let bios_files = [
        "pxelinux.0",
        "lpxelinux.0",
    ];

    let efi_files = [
        "grubnetx64.efi.signed",
        "grubx64.efi",
        "bootnetx64.efi",
        "shimx64.efi.signed",
    ];

    let bios = bios_files
        .iter()
        .find(|f| root.join(f).exists())
        .unwrap_or(&"pxelinux.0");

    let efi = efi_files
        .iter()
        .find(|f| root.join(f).exists())
        .unwrap_or(&"grubnetx64.efi.signed");

    (bios.to_string(), efi.to_string())
}
