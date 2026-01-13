# Serabut

A PXE boot server that automatically downloads Ubuntu netboot images and serves them to PXE clients using proxyDHCP. Works alongside your existing DHCP server.

## Features

- **Automatic netboot download**: Fetches Ubuntu 24.04 netboot images from releases.ubuntu.com
- **SHA256 verification**: Verifies downloads against official checksums
- **TFTP server**: Built-in TFTP server for serving boot files
- **ProxyDHCP**: Works with existing DHCP servers - no need to replace your router
- **PXE monitoring**: Real-time logging of all PXE boot activity
- **Multi-architecture**: Supports both BIOS and UEFI clients

## How It Works

1. **Startup**: Downloads and verifies the latest Ubuntu netboot image
2. **TFTP Server**: Serves boot files (pxelinux.0, grubnetx64.efi.signed, etc.)
3. **ProxyDHCP**: Responds to PXE clients with boot server info
4. **Monitor**: Logs all PXE boot requests and responses

The proxyDHCP approach means your existing DHCP server (router) handles IP assignment while Serabut provides PXE boot information.

## Requirements

- Rust 1.70+
- Linux (requires raw socket access)
- Root/sudo privileges
- Network access to releases.ubuntu.com

## Building

```bash
# Release build (recommended)
cargo build --release
```

## Running

### Full PXE Boot Server

```bash
# Auto-detect interface and server IP
sudo ./target/release/serabut

# Specify server IP explicitly
sudo ./target/release/serabut --server-ip 192.168.1.100

# Use custom data directory
sudo ./target/release/serabut --data-dir /opt/pxe
```

### Monitor Only Mode

```bash
# Just monitor PXE traffic without serving files
sudo ./target/release/serabut --monitor-only
```

### Skip Download

```bash
# Use existing netboot files (skip download check)
sudo ./target/release/serabut --skip-download
```

## Command Line Options

| Option | Description |
|--------|-------------|
| `-i, --interface <NAME>` | Network interface to listen on (default: auto-detect) |
| `--server-ip <IP>` | Server IP address for TFTP/proxyDHCP |
| `--data-dir <PATH>` | Directory for netboot files (default: /var/lib/serabut) |
| `--tftp-port <PORT>` | TFTP server port (default: 69) |
| `--skip-download` | Skip netboot download, use existing files |
| `--monitor-only` | Monitor only mode, no TFTP/proxyDHCP servers |
| `-v, --verbose` | Enable verbose output |
| `--no-color` | Disable colored output |
| `--list-interfaces` | List available network interfaces and exit |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

## Example Output

```
2024-01-15T10:30:00 INFO  === Checking for latest Ubuntu netboot image ===
2024-01-15T10:30:01 INFO  Remote SHA256: abc123...
2024-01-15T10:30:01 INFO  Netboot image is up to date
2024-01-15T10:30:01 INFO  === Starting TFTP server ===
2024-01-15T10:30:01 INFO  TFTP server listening on 0.0.0.0:69
2024-01-15T10:30:01 INFO  === Starting proxyDHCP server ===
2024-01-15T10:30:01 INFO  ProxyDHCP server listening on ports 67 and 4011
2024-01-15T10:30:01 INFO  === Starting PXE boot monitor ===

[PXE DISCOVER] MAC: AA:BB:CC:DD:EE:FF | XID: 0x12345678 | Arch: EFI x64
2024-01-15T10:30:05 INFO  PXE OFFER sent to AA:BB:CC:DD:EE:FF -> boot file: grubnetx64.efi.signed
[PXE OFFER]    MAC: AA:BB:CC:DD:EE:FF | IP: 192.168.1.100 | Server: 192.168.1.1 | XID: 0x12345678 | Arch: EFI x64
2024-01-15T10:30:05 INFO  TFTP: AA:BB:CC:DD:EE:FF requesting grubnetx64.efi.signed
2024-01-15T10:30:06 INFO  TFTP: Transfer complete: grubnetx64.efi.signed (1234567 bytes)
```

## Directory Structure

```
/var/lib/serabut/
├── ubuntu-24.04.2-netboot-amd64.tar.gz   # Downloaded tarball
└── tftp/                                  # TFTP root directory
    ├── pxelinux.0                        # BIOS boot file
    ├── grubnetx64.efi.signed             # UEFI boot file
    ├── grub/                             # GRUB configuration
    └── ...                               # Other netboot files
```

## Firewall Configuration

Ensure these ports are open:

| Port | Protocol | Service |
|------|----------|---------|
| 67 | UDP | DHCP/ProxyDHCP |
| 69 | UDP | TFTP |
| 4011 | UDP | ProxyDHCP (alternate) |

## Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture
```

## Troubleshooting

### "Permission denied" errors
Run with sudo - raw socket access requires root privileges.

### "Address already in use" on port 67
Another DHCP server might be running. Use `--monitor-only` to just observe traffic.

### Client not booting
1. Check firewall allows UDP ports 67, 69, 4011
2. Verify client is set to PXE/network boot in BIOS
3. Check `--verbose` output for details

### Download fails
Ensure network access to https://releases.ubuntu.com/24.04/

## License

MIT
