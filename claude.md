# Serabut - Lightweight Bare Metal Provisioning

A simple PXE boot discovery and provisioning tool, similar to MAAS/Cobbler/Foreman but lightweight.

## Project Overview

Two executables:
- `serabut` - CLI tool for managing discovered machines and boot assignments
- `serabutd` - Daemon that handles ProxyDHCP and HTTP endpoints

## Architecture

### Data Files

**`/var/lib/serabut/mac.txt`** - Discovered machines (CSV)
```
label,mac,last_seen
dbnode,aa:bb:cc:dd:ee:ff,2026-01-15T19:30:00Z
,11:22:33:44:55:66,2026-01-15T18:45:00Z
```
- label: a-z only, max 8 chars, unique, can be empty
- mac: standard format aa:bb:cc:dd:ee:ff
- last_seen: ISO 8601 timestamp

**`/var/lib/serabut/boot.txt`** - Active boot assignments (CSV)
```
mac,profile,assigned_at
aa:bb:cc:dd:ee:ff,ubuntu-24.04,2026-01-15T20:00:00Z
```

**`/etc/serabut/profiles/*.ipxe`** - Boot profiles
```
/etc/serabut/profiles/
├── ubuntu-24.04.ipxe
├── ubuntu-22.04.ipxe
├── rocky-9.ipxe
```

### CLI Commands

```
serabut mac list                      # list all, sorted by last seen desc
serabut mac label <mac> <label>       # assign label (errors: MAC not found, label taken by X, invalid format)
serabut mac label <mac> ""            # clear label
serabut mac remove <mac>              # delete entry

serabut boot add <label|mac> <profile>  # assign boot profile (validates profile exists)
serabut boot remove <label|mac>         # remove assignment
serabut boot list                       # show active assignments

serabut profiles list                   # list available profiles from /etc/serabut/profiles/
```

### Daemon (serabutd)

**Port 67 (UDP) - ProxyDHCP**
- Listens for PXE DHCP DISCOVER packets
- Two response types based on client:
  - PXE ROM (option 60: PXEClient, no option 77: iPXE) → respond with TFTP server + filename (ipxe.efi)
  - iPXE (option 77: iPXE) → respond with option 175 containing script URL

**Port 6007 (TCP) - HTTP**
- `GET /boot?mac=xx:xx:xx:xx:xx:xx` - Return iPXE script based on assignment
- `GET /done?mac=xx:xx:xx:xx:xx:xx` - Phone-home endpoint, removes assignment from boot.txt

### PXE Boot Flow

```
1. Machine PXE boots
2. Real DHCP server gives IP
3. serabutd (ProxyDHCP) responds: "get ipxe.efi from TFTP"
4. iPXE loads, sends DHCP again (option 77: iPXE)
5. serabutd responds with option 175: script URL
6. iPXE fetches http://server:6007/boot?mac=...
7. serabutd returns script:
   - No assignment → "#!ipxe\nexit" (boot local)
   - Has assignment → kernel/initrd/boot commands for that profile
8. After install, phone-home: http://server:6007/done?mac=...
9. serabutd removes from boot.txt
10. Next reboot → boots local
```

### Validation Rules

- `serabut mac label`: MAC must exist in mac.txt, label must be unique, a-z only, max 8 chars
- `serabut boot add`: profile must exist in /etc/serabut/profiles/<profile>.ipxe
- Label errors show who owns it: "Label 'foo' already taken by aa:bb:cc:dd:ee:ff"

## Development

### Project Structure (single crate, multiple binaries)
```
serabut/
├── Cargo.toml
├── src/
│   ├── lib.rs           # shared: MAC parsing, file I/O, validation
│   ├── bin/
│   │   ├── serabut.rs   # CLI
│   │   └── serabutd.rs  # daemon
```

### Environment Variables

Paths are configurable for testing without root:
- `SERABUT_DATA_DIR` - data directory (default: `/var/lib/serabut`)
- `SERABUT_CONFIG_DIR` - config directory (default: `/etc/serabut`)

Example:
```bash
SERABUT_DATA_DIR=/tmp/serabut ./target/debug/serabut mac list
```

### Running

```bash
# Build
cargo build

# Run daemon (needs root for raw sockets)
sudo ./target/debug/serabutd -i eth0

# CLI commands
./target/debug/serabut mac list
./target/debug/serabut mac label aa:bb:cc:dd:ee:ff mynode
./target/debug/serabut profiles list
```

### Testing

```bash
cargo test
```

65 unit tests covering:
- MAC/label validation
- CSV parsing and roundtrip
- Entry lookup (by MAC, by label)
- File I/O with temp directories
- DHCP packet parsing
- PXE/iPXE detection
- RFC 2132 DHCP message type constants (DISCOVER=1, OFFER=2, REQUEST=3)

### MVP Roadmap

**MVP 1 - Discovery**
- serabutd listens for PXE DHCP, logs MACs
- serabut mac list/label/remove

**MVP 2 - Boot Assignments**
- serabut boot add/remove/list
- serabut profiles list
- serabutd HTTP endpoints (/boot, /done)
- ProxyDHCP responses (TFTP + iPXE script URL)

**MVP 3 - Full Boot**
- Ubuntu 24.04 profile with autoinstall
- Additional OS profiles

## External Dependencies

- Existing DHCP server (serabut doesn't serve leases)
- TFTP server with iPXE files (/srv/tftp/ipxe.efi, undionly.kpxe)
- HTTP server for OS kernel/initrd files (or serve from serabutd later)
