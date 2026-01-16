# Serabut

Lightweight bare metal PXE provisioning tool. Similar to MAAS/Cobbler/Foreman but simpler.

## Overview

Serabut provides two executables:

- **`serabut`** - CLI tool for managing discovered machines and boot assignments
- **`serabutd`** - Daemon that listens for PXE boot requests and records MAC addresses

## Requirements

- Rust 1.70+
- Linux (requires raw socket access)
- Root/sudo privileges or CAP_NET_RAW capability for `serabutd`

## Quick Start

```bash
# Build
cargo build --release

# Start the daemon (requires root)
sudo ./target/release/serabutd -i eth0

# In another terminal, manage discovered machines
./target/release/serabut mac list
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

Listens on a network interface for PXE DHCP DISCOVER packets and records client MAC addresses.

### Usage

```bash
# Auto-detect interface
sudo ./target/release/serabutd

# Specify interface
sudo ./target/release/serabutd -i eth0
sudo ./target/release/serabutd --interface enp0s3
```

### Options

| Option | Description |
|--------|-------------|
| `-i, --interface <NAME>` | Network interface to listen on (default: auto-detect) |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

### What it does

1. Captures network packets on the specified interface
2. Filters for DHCP DISCOVER packets with PXE vendor class (`PXEClient`)
3. Records the MAC address and timestamp to `/var/lib/serabut/mac.txt`
4. Logs PXE boot requests to stderr

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

### Boot Assignments (MVP 2 - Coming Soon)

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
```

## Project Structure

```
serabut/
├── src/
│   ├── lib.rs           # Shared library: MAC parsing, file I/O, validation
│   └── bin/
│       ├── serabut.rs   # CLI tool
│       └── serabutd.rs  # Daemon
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
3. serabutd detects PXE DISCOVER, records MAC
4. (MVP 2) serabutd responds with boot file location
5. (MVP 2) Machine fetches boot script from serabutd HTTP endpoint
6. (MVP 2) Machine boots assigned profile or exits to local boot
```

## Validation Rules

- **MAC addresses**: Format `aa:bb:cc:dd:ee:ff` (case-insensitive, stored lowercase)
- **Labels**: `a-z` only, max 8 characters, must be unique

## Roadmap

### MVP 1 - Discovery (Current)
- [x] serabutd listens for PXE DHCP, logs MACs
- [x] serabut mac list/label/remove

### MVP 2 - Boot Assignments
- [ ] serabut boot add/remove/list
- [ ] serabutd HTTP endpoints (/boot, /done)
- [ ] ProxyDHCP responses

### MVP 3 - Full Boot
- [ ] Ubuntu 24.04 profile with autoinstall
- [ ] Additional OS profiles

## External Dependencies

Serabut is designed to work alongside existing infrastructure:

- **DHCP server** - Serabut doesn't serve IP leases
- **TFTP server** - For iPXE chainloading (e.g., `/srv/tftp/ipxe.efi`)
- **HTTP server** - For OS kernel/initrd files (or served by serabutd in MVP 2+)

## License

MIT
