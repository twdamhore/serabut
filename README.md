# Serabut

A DHCP PXE boot listener that monitors network traffic and reports PXE boot requests.

## Features

- Monitors DHCP traffic for PXE boot activity
- Reports client MAC addresses and assigned IP addresses
- Detects client architecture (BIOS, EFI x64, EFI ARM64, etc.)
- Supports verbose mode with UUID and vendor class details
- Colored terminal output (can be disabled)
- Graceful shutdown with Ctrl+C

## Requirements

- Rust 1.70+
- Linux (requires raw socket access)
- Root/sudo privileges for packet capture

## Quick Start

```bash
# Clone the repository
git clone https://github.com/twdamhore/serabut.git
cd serabut

# Build release binary
cargo build --release

# Run with auto-detected interface (requires root)
sudo ./target/release/serabut
```

## Building

### Debug Build

```bash
cargo build
```

The debug binary will be at `./target/debug/serabut`.

### Release Build (Recommended)

```bash
cargo build --release
```

The release binary will be at `./target/release/serabut`.

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run a specific test
cargo test test_name

# Run tests for a specific module
cargo test parser::

# Run coverage (requires cargo-tarpaulin)
cargo install cargo-tarpaulin
cargo tarpaulin
```

## Running

The application requires root privileges to capture raw network packets.

### Basic Usage

```bash
# Run on auto-detected interface
sudo ./target/release/serabut

# Run on specific interface
sudo ./target/release/serabut -i eth0

# Verbose mode (shows UUID and vendor class)
sudo ./target/release/serabut -v

# Combine options
sudo ./target/release/serabut -i eth0 -v --no-color
```

### List Available Interfaces

```bash
# No root required for listing interfaces
./target/release/serabut --list-interfaces
```

## Command Line Options

| Option | Description |
|--------|-------------|
| `-i, --interface <NAME>` | Network interface to listen on (default: auto-detect) |
| `-v, --verbose` | Enable verbose output |
| `--no-color` | Disable colored output |
| `--list-interfaces` | List available network interfaces and exit |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

## Example Output

### Standard Output

```
Listening for PXE boot requests on interface: eth0
Press Ctrl+C to stop.

[PXE DISCOVER] MAC: AA:BB:CC:DD:EE:FF | XID: 0x12345678 | Arch: EFI x64
[PXE OFFER]    MAC: AA:BB:CC:DD:EE:FF | IP: 192.168.1.100 | Server: 192.168.1.1 | XID: 0x12345678 | Arch: EFI x64
[PXE REQUEST]  MAC: AA:BB:CC:DD:EE:FF | XID: 0x12345678 | Arch: EFI x64
[PXE ACK]      MAC: AA:BB:CC:DD:EE:FF | IP: 192.168.1.100 | Server: 192.168.1.1 | XID: 0x12345678 | Arch: EFI x64
```

### Verbose Output

```
[PXE DISCOVER] MAC: AA:BB:CC:DD:EE:FF | XID: 0x12345678 | Arch: EFI x64
               UUID: 12345678-1234-1234-1234-123456789ABC
               Vendor: PXEClient:Arch:00007:UNDI:003016
```

## Project Structure

```
serabut/
├── src/
│   ├── main.rs              # CLI entry point and argument parsing
│   ├── lib.rs               # Core PxeListener orchestrator
│   ├── error.rs             # Error types and handling
│   ├── domain/              # Business logic types
│   │   ├── mod.rs
│   │   ├── dhcp.rs          # DHCP protocol types
│   │   ├── pxe.rs           # PXE-specific types
│   │   └── events.rs        # PxeBootEvent domain events
│   ├── parser/              # DHCP packet parsing
│   │   ├── mod.rs
│   │   └── dhcp_parser.rs   # RFC 2131 compliant parser
│   ├── detector/            # PXE detection logic
│   │   ├── mod.rs
│   │   └── pxe_detector.rs
│   ├── capture/             # Network packet capture
│   │   ├── mod.rs
│   │   └── pnet_capture.rs  # pnet-based implementation
│   └── reporter/            # Event reporting
│       ├── mod.rs
│       └── console_reporter.rs
├── Cargo.toml
├── Cargo.lock
├── README.md
└── LICENSE
```

## Architecture

The project follows **SOLID principles** with a clean, modular architecture:

- **Single Responsibility**: Each module handles one concern
- **Open/Closed**: Extensible through traits without modifying existing code
- **Liskov Substitution**: All trait implementations are interchangeable
- **Interface Segregation**: Focused, minimal trait definitions
- **Dependency Inversion**: High-level modules depend on abstractions

### Core Traits

| Trait | Purpose |
|-------|---------|
| `PacketCapture` | Network packet capture abstraction |
| `EventReporter` | Event reporting interface |
| `DhcpParser` | DHCP packet parsing |
| `PxeDetector` | PXE boot detection |

## Supported Architectures

| Architecture | Code | Description |
|--------------|------|-------------|
| BIOS | 0x00 | Legacy BIOS x86 |
| EFI x86 | 0x06 | EFI 32-bit |
| EFI x64 | 0x07 | EFI 64-bit (most common) |
| EFI BC | 0x09 | EFI Byte Code |
| EFI ARM32 | 0x0A | EFI ARM 32-bit |
| EFI ARM64 | 0x0B | EFI ARM 64-bit |
| EFI x64 HTTP | 0x10 | EFI x64 HTTP Boot |

## Troubleshooting

### Permission Denied

```
Error: Operation not permitted
```

**Solution**: Run with sudo or as root:
```bash
sudo ./target/release/serabut
```

### No Interface Found

```
Error: No suitable network interface found
```

**Solution**: Specify the interface manually:
```bash
# List available interfaces
./target/release/serabut --list-interfaces

# Use a specific interface
sudo ./target/release/serabut -i <interface-name>
```

### No PXE Traffic Detected

- Ensure you're on the same network segment as the PXE clients
- Check that DHCP traffic isn't being blocked by firewalls
- Verify the correct interface is selected
- Use Wireshark to confirm DHCP traffic is present

## Dependencies

| Crate | Purpose |
|-------|---------|
| pnet | Network packet capture and parsing |
| clap | Command-line argument parsing |
| thiserror | Error type definitions |
| anyhow | Error handling utilities |
| tracing | Structured logging |
| macaddr | MAC address handling |
| ctrlc | Signal handling for graceful shutdown |

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests (`cargo test`)
5. Commit your changes (`git commit -m 'Add amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request

## License

MIT
