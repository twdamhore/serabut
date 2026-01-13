# Serabut

A DHCP PXE boot listener that monitors network traffic and reports PXE boot requests.

## Features

- Monitors DHCP traffic for PXE boot activity
- Reports client MAC addresses and assigned IP addresses
- Detects client architecture (BIOS, EFI x64, EFI ARM64, etc.)
- Supports verbose mode with UUID and vendor class details

## Requirements

- Rust 1.70+
- Linux (requires raw socket access)
- Root/sudo privileges for packet capture

## Building

```bash
# Debug build
cargo build

# Release build (recommended)
cargo build --release
```

## Running

The application requires root privileges to capture raw network packets.

```bash
# Run on auto-detected interface
sudo ./target/release/serabut

# Run on specific interface
sudo ./target/release/serabut -i eth0

# Verbose mode (shows UUID and vendor class)
sudo ./target/release/serabut -v

# List available interfaces
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

```
Listening for PXE boot requests on interface: eth0
Press Ctrl+C to stop.

[PXE DISCOVER] MAC: AA:BB:CC:DD:EE:FF | XID: 0x12345678 | Arch: EFI x64
[PXE OFFER]    MAC: AA:BB:CC:DD:EE:FF | IP: 192.168.1.100 | Server: 192.168.1.1 | XID: 0x12345678 | Arch: EFI x64
[PXE REQUEST]  MAC: AA:BB:CC:DD:EE:FF | XID: 0x12345678 | Arch: EFI x64
[PXE ACK]      MAC: AA:BB:CC:DD:EE:FF | IP: 192.168.1.100 | Server: 192.168.1.1 | XID: 0x12345678 | Arch: EFI x64
```

## Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run coverage (requires cargo-tarpaulin)
cargo install cargo-tarpaulin
cargo tarpaulin
```

## License

MIT
