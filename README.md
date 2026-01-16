# Serabut

Lightweight bare metal PXE provisioning tool. Similar to MAAS/Cobbler/Foreman but simpler.

## Overview

Serabut provides two executables:

- **`serabut`** - CLI tool for managing discovered machines and boot assignments
- **`serabutd`** - Daemon that handles ProxyDHCP responses and serves iPXE boot scripts

## Requirements

- Rust 1.70+
- Linux (requires raw socket access)
- Root/sudo privileges or CAP_NET_RAW capability for `serabutd`

## Quick Start

```bash
# Build
cargo build --release

# Start the daemon (requires root)
sudo ./target/release/serabutd -i br0

# In another terminal, manage discovered machines
./target/release/serabut mac list

# Assign a boot profile
./target/release/serabut mac label 52:54:00:26:10:02 mynode
./target/release/serabut boot add mynode ubuntu
```

## Building

```bash
# Debug build
cargo build

# Release build (recommended)
cargo build --release
```

Binaries will be at:
- `./target/release/serabut`
- `./target/release/serabutd`

## serabutd - The Daemon

PXE boot server with ProxyDHCP and HTTP endpoints.

### Usage

```bash
# Auto-detect interface
sudo ./target/release/serabutd

# Specify interface
sudo ./target/release/serabutd -i br0

# Custom HTTP port and boot file
sudo ./target/release/serabutd -i br0 --http-port 8080 --boot-file undionly.kpxe

# Listen-only mode (no ProxyDHCP responses)
sudo ./target/release/serabutd -i br0 --no-respond
```

### Options

| Option | Description |
|--------|-------------|
| `-i, --interface <NAME>` | Network interface to listen on (default: auto-detect) |
| `--http-port <PORT>` | HTTP server port (default: 6007) |
| `--boot-file <FILE>` | TFTP boot filename for PXE ROM clients (default: ipxe.efi) |
| `--no-respond` | Disable ProxyDHCP responses (listen-only mode) |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

### What it does

1. Captures network packets on the specified interface
2. Filters for DHCP DISCOVER/REQUEST packets with PXE vendor class
3. Records MAC addresses to `/var/lib/serabut/mac.txt`
4. Sends ProxyDHCP responses:
   - **PXE ROM clients**: TFTP server + boot filename (ipxe.efi)
   - **iPXE clients**: Boot script URL via option 175
5. Serves HTTP endpoints for iPXE scripts

### HTTP Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /boot?mac=...` | Returns iPXE script based on boot assignment |
| `GET /done?mac=...` | Phone-home endpoint, removes boot assignment |
| `GET /health` | Health check, returns "ok" |

## serabut - The CLI

Command-line tool for managing discovered machines, boot assignments, and profiles.

### MAC Address Management

```bash
# List all discovered MAC addresses (sorted by last seen)
serabut mac list

# Assign a label to a MAC address (a-z only, max 8 chars)
serabut mac label aa:bb:cc:dd:ee:ff dbnode

# Clear a label
serabut mac label aa:bb:cc:dd:ee:ff ""

# Remove a MAC address from the list
serabut mac remove aa:bb:cc:dd:ee:ff
```

### Boot Assignments

```bash
# Assign a boot profile to a machine (by MAC or label)
serabut boot add dbnode ubuntu-24.04
serabut boot add aa:bb:cc:dd:ee:ff rocky-9

# Remove a boot assignment
serabut boot remove dbnode

# List active boot assignments
serabut boot list
```

### Profile Management

```bash
# List available boot profiles
serabut profiles list
```

## Data Files

| File | Purpose |
|------|---------|
| `/var/lib/serabut/mac.txt` | Discovered MAC addresses (CSV: label,mac,timestamp) |
| `/var/lib/serabut/boot.txt` | Active boot assignments (CSV: mac,profile,timestamp) |
| `/etc/serabut/profiles/*.ipxe` | Boot profile scripts |

### Example mac.txt

```
dbnode,aa:bb:cc:dd:ee:ff,2026-01-15T19:30:00Z
,11:22:33:44:55:66,2026-01-15T18:45:00Z
```

### Example boot.txt

```
aa:bb:cc:dd:ee:ff,ubuntu-24.04,2026-01-15T20:00:00Z
```

### Example Profile (ubuntu-24.04.ipxe)

```
#!ipxe
kernel http://archive.ubuntu.com/ubuntu/dists/noble/main/installer-amd64/current/images/netboot/ubuntu-installer/amd64/linux
initrd http://archive.ubuntu.com/ubuntu/dists/noble/main/installer-amd64/current/images/netboot/ubuntu-installer/amd64/initrd.gz
boot
```

## Environment Variables

Paths are configurable for testing without root:

| Variable | Default | Description |
|----------|---------|-------------|
| `SERABUT_DATA_DIR` | `/var/lib/serabut` | Data directory for mac.txt, boot.txt |
| `SERABUT_CONFIG_DIR` | `/etc/serabut` | Config directory for profiles |

Example:
```bash
SERABUT_DATA_DIR=/tmp/serabut ./target/debug/serabut mac list
```

## Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test validate_mac
cargo test dhcp
cargo test boot_entry
```

## Project Structure

```
serabut/
├── src/
│   ├── lib.rs           # Shared library: MAC/boot entry parsing, file I/O, validation
│   └── bin/
│       ├── serabut.rs   # CLI tool
│       └── serabutd.rs  # Daemon (ProxyDHCP + HTTP server)
├── Cargo.toml
├── Cargo.lock
├── README.md
├── claude.md            # Development notes
└── LICENSE
```

## PXE Boot Flow

```
1. Machine PXE boots
2. Real DHCP server assigns IP
3. serabutd detects PXE DISCOVER, records MAC, sends ProxyDHCP OFFER
4. PXE ROM loads ipxe.efi from TFTP server
5. iPXE sends DHCP DISCOVER with option 77 (iPXE)
6. serabutd responds with boot script URL in option 175
7. iPXE fetches http://server:6007/boot?mac=...
8. serabutd returns iPXE script based on boot.txt assignment
   - No assignment → "#!ipxe\nexit" (boot local)
   - Has assignment → profile script (kernel/initrd/boot)
9. After install, phone-home: http://server:6007/done?mac=...
10. serabutd removes from boot.txt, next boot goes local
```

## Validation Rules

- **MAC addresses**: Format `aa:bb:cc:dd:ee:ff` (case-insensitive, stored lowercase)
- **Labels**: `a-z` only, max 8 characters, must be unique

## Roadmap

### MVP 1 - Discovery
- [x] serabutd listens for PXE DHCP, logs MACs
- [x] serabut mac list/label/remove

### MVP 2 - Boot Assignments (Current)
- [x] serabut boot add/remove/list
- [x] serabutd HTTP endpoints (/boot, /done)
- [x] ProxyDHCP responses (PXE ROM and iPXE)

### MVP 3 - Full Boot
- [ ] Ubuntu 24.04 profile with autoinstall
- [ ] Additional OS profiles
- [ ] Built-in TFTP server (optional)

## External Dependencies

Serabut is designed to work alongside existing infrastructure:

- **DHCP server** - Serabut doesn't serve IP leases (ProxyDHCP only)
- **TFTP server** - For iPXE chainloading (e.g., `/srv/tftp/ipxe.efi`)

## License

MIT
