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
use serabut::http::CloudInitServer;
use serabut::netboot::{AutoinstallConfig, BootloaderConfigGenerator, NetbootConfigs, NetbootManager};
use serabut::proxydhcp::ProxyDhcpServer;
use serabut::reporter::ConsoleReporter;
use serabut::tftp::TftpServer;
use serabut::PxeListener;

/// DHCP PXE Boot Server
#[derive(Parser, Debug)]
#[command(author, version, about = "PXE boot server for netboot images", long_about = None)]
struct Args {
    /// Network interface to listen on (required for server mode)
    #[arg(short, long)]
    interface: Option<String>,

    /// Operating system to serve (default: ubuntu-24.04)
    /// Use --list-os to see available options
    #[arg(long, default_value = "ubuntu-24.04")]
    os: String,

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

    /// List available operating systems and exit
    #[arg(long)]
    list_os: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Enable Ubuntu autoinstall with cloud-init
    /// Starts HTTP server and configures bootloader for autoinstall
    #[arg(long)]
    autoinstall: bool,

    /// Path to user-data file for autoinstall (cloud-init format)
    #[arg(long)]
    user_data: Option<PathBuf>,

    /// Port for cloud-init HTTP server (default: 8080)
    #[arg(long, default_value = "8080")]
    http_port: u16,
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

    // Handle --list-os
    if args.list_os {
        println!("Available operating systems:\n");
        for config in NetbootConfigs::list() {
            println!("  {:15} - {}", config.id, config.name);
        }
        println!("\nUse --os <id> to select an operating system.");
        return;
    }

    // Get netboot configuration
    let netboot_config = match NetbootConfigs::get(&args.os) {
        Some(config) => config,
        None => {
            eprintln!("Error: Unknown operating system '{}'", args.os);
            eprintln!("\nAvailable options:");
            for id in NetbootConfigs::available_ids() {
                eprintln!("  {}", id);
            }
            eprintln!("\nUse --list-os to see full descriptions.");
            process::exit(1);
        }
    };

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
    let (tftp_root, boot_file_bios, boot_file_efi) = if !args.skip_download && !args.monitor_only {
        info!("=== Preparing {} netboot image ===", netboot_config.name);

        let manager = NetbootManager::new(&args.data_dir, netboot_config.clone());
        match manager.ensure_netboot_ready() {
            Ok(root) => {
                info!("Netboot files ready at: {}", root.display());
                let bios = manager.config().boot_file_bios.clone();
                let efi = manager.config().boot_file_efi.clone();
                (Some(root), bios, efi)
            }
            Err(e) => {
                error!("Failed to prepare netboot image: {}", e);
                eprintln!("\nError: Could not prepare netboot image.");
                eprintln!("Use --skip-download to use existing files.");
                eprintln!("Use --monitor-only to just monitor PXE traffic.");
                process::exit(1);
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
        // Use boot files from config, but also check what's available
        let (bios, efi) = detect_boot_files(&tftp_root);
        (Some(tftp_root), bios, efi)
    } else {
        // Monitor only mode
        (None, netboot_config.boot_file_bios.clone(), netboot_config.boot_file_efi.clone())
    };

    info!("BIOS boot file: {}", boot_file_bios);
    info!("EFI boot file: {}", boot_file_efi);

    // Step 2: Set up autoinstall if enabled
    let http_handle = if args.autoinstall && !args.monitor_only {
        info!("=== Configuring autoinstall ===");

        // Build HTTP server URL
        let http_addr = SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            args.http_port,
        ));

        // Create cloud-init data directory
        let cloud_init_dir = args.data_dir.join("cloud-init");
        if !cloud_init_dir.exists() {
            std::fs::create_dir_all(&cloud_init_dir).expect("Failed to create cloud-init directory");
        }

        // Build autoinstall URL using server IP
        let autoinstall_url = format!("http://{}:{}/", server_ip, args.http_port);
        info!("Autoinstall datasource URL: {}", autoinstall_url);

        // Create autoinstall config
        let autoinstall_config = AutoinstallConfig::new(&autoinstall_url);

        // Generate bootloader configs with autoinstall parameters
        if let Some(ref root) = tftp_root {
            let generator = BootloaderConfigGenerator::new(root)
                .with_autoinstall(autoinstall_config);

            if let Err(e) = generator.generate() {
                warn!("Failed to generate bootloader configs: {}", e);
            } else {
                info!("Generated bootloader configs with autoinstall parameters");
            }
        }

        // Create HTTP server
        let mut http_server = CloudInitServer::new(&cloud_init_dir, http_addr);

        // Load user-data if provided
        if let Some(ref user_data_path) = args.user_data {
            if let Err(e) = http_server.load_user_data(user_data_path) {
                warn!("Failed to load user-data: {}", e);
            } else {
                info!("Loaded user-data from: {}", user_data_path.display());
            }
        }

        let http_running = http_server.running_flag();
        let global_running = running.clone();

        info!("=== Starting cloud-init HTTP server ===");
        let handle = thread::spawn(move || {
            if let Err(e) = http_server.run() {
                error!("HTTP server error: {}", e);
            }
        });

        // Link HTTP running to global running
        let http_running_clone = http_running.clone();
        thread::spawn(move || {
            while global_running.load(Ordering::SeqCst) {
                thread::sleep(std::time::Duration::from_millis(100));
            }
            http_running_clone.store(false, Ordering::SeqCst);
        });

        Some(handle)
    } else {
        None
    };

    // Step 3: Start TFTP server (unless monitor-only)
    #[allow(clippy::manual_map)]
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

    // Step 4: Start proxyDHCP server (unless monitor-only)
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

    // Step 5: Start PXE monitor
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
    if let Some(handle) = http_handle {
        let _ = handle.join();
    }
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
