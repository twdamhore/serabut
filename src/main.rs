//! Serabut - DHCP PXE Boot Listener
//!
//! Listens for DHCP PXE boot requests on the network and reports
//! the MAC address of clients and the IPs assigned by DHCP servers.

use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use serabut::capture::PnetCapture;
use serabut::reporter::ConsoleReporter;
use serabut::PxeListener;

/// DHCP PXE Boot Listener
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Network interface to listen on (default: auto-detect)
    #[arg(short, long)]
    interface: Option<String>,

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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("serabut=info".parse().unwrap()),
        )
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

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
        println!("\nReceived interrupt signal, shutting down...");
    })
    .expect("Error setting Ctrl-C handler");

    // Create and run the listener
    let mut listener = PxeListener::new(capture, reporter);

    if let Err(e) = listener.run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
