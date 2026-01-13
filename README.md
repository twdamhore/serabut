# Serabut

A PXE boot server that automatically downloads netboot images and serves them to PXE clients using proxyDHCP. Works alongside your existing DHCP server.

## Features

- **Multiple OS support**: Ubuntu, Debian, Rocky Linux, AlmaLinux (extensible)
- **Automatic netboot download**: Fetches netboot images from official sources
- **SHA256 verification**: Verifies downloads against official checksums
- **TFTP server**: Built-in TFTP server for serving boot files
- **ProxyDHCP**: Works with existing DHCP servers - no need to replace your router
- **PXE monitoring**: Real-time logging of all PXE boot activity
- **Multi-architecture**: Supports both BIOS and UEFI clients
- **Ubuntu autoinstall**: Automated Ubuntu installations with cloud-init

## Supported Operating Systems

| ID | Name |
|----|------|
| `ubuntu-24.04` | Ubuntu 24.04 LTS (Noble Numbat) |
| `ubuntu-22.04` | Ubuntu 22.04 LTS (Jammy Jellyfish) |
| `debian-12` | Debian 12 (Bookworm) |
| `rocky-9` | Rocky Linux 9 |
| `rocky-10` | Rocky Linux 10 |
| `alma-9` | AlmaLinux 9 |
| `alma-10` | AlmaLinux 10 |

Use `--list-os` to see all available options.

## How It Works

1. **Startup**: Downloads and verifies the selected netboot image
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

## Quick Start

```bash
# Build
cargo build --release

# Start PXE server for Ubuntu 24.04 on eth0
sudo ./target/release/serabut -i eth0

# That's it! Boot any machine on your network via PXE.
```

## Examples

### Example 1: Basic Ubuntu 24.04 Install

The simplest case - serve Ubuntu 24.04 installer to PXE clients:

```bash
sudo ./target/release/serabut -i eth0
```

On first run, this will:
1. Download Ubuntu 24.04 netboot image (~400MB)
2. Verify SHA256 checksum
3. Extract to `/var/lib/serabut/tftp/`
4. Start TFTP and proxyDHCP servers

Then PXE boot any machine and select "Ubuntu Server Install" from the menu.

### Example 2: Rocky Linux 9 Install

```bash
sudo ./target/release/serabut -i eth0 --os rocky-9
```

### Example 3: Debian 12 Install

```bash
sudo ./target/release/serabut -i eth0 --os debian-12
```

### Example 4: AlmaLinux 10 Install

```bash
sudo ./target/release/serabut -i eth0 --os alma-10
```

### Example 5: Automated Ubuntu Install (No User Input)

Create a `user-data.yaml` file for fully automated installation:

```yaml
#cloud-config
autoinstall:
  version: 1
  locale: en_US.UTF-8
  keyboard:
    layout: us
  identity:
    hostname: ubuntu-server
    username: admin
    # Password: "ubuntu" - generate with: mkpasswd -m sha-512
    password: "$6$xyz$LnMaTH0tLrKR9K0OrLDjCk2E3k8EvRWuGd.mGVSBKnOLKQ8dF7jR7t/Kp7N8E3X7W5lE0Qr6VJ2Rt9w2QJ8A0"
  ssh:
    install-server: true
    allow-pw: true
    authorized-keys:
      - ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI... your-key-here
  storage:
    layout:
      name: lvm
  late-commands:
    - echo 'admin ALL=(ALL) NOPASSWD:ALL' > /target/etc/sudoers.d/admin
```

Run with autoinstall:

```bash
sudo ./target/release/serabut -i eth0 --autoinstall --user-data user-data.yaml
```

The machine will install Ubuntu completely unattended and reboot when done.

### Example 6: Minimal Autoinstall (Prompt for Disk Only)

```yaml
#cloud-config
autoinstall:
  version: 1
  interactive-sections:
    - storage
  identity:
    hostname: ubuntu-server
    username: ubuntu
    password: "$6$xyz$hash..."
  ssh:
    install-server: true
```

### Example 7: Monitor PXE Traffic Only

Watch PXE boot activity without serving any files:

```bash
sudo ./target/release/serabut -i eth0 --monitor-only
```

Output:
```
[PXE DISCOVER] MAC: E8:FF:1E:D5:97:54 | XID: 0xd5c9b850 | Arch: EFI x64
[PXE OFFER]    MAC: E8:FF:1E:D5:97:54 | IP: 192.168.4.167 | Server: 192.168.4.1 | XID: 0xd5c9b850
```

### Example 8: Use Existing Files (Offline Mode)

If you've already downloaded the netboot image:

```bash
sudo ./target/release/serabut -i eth0 --skip-download
```

### Example 9: Custom Data Directory

Store netboot files in a custom location:

```bash
sudo ./target/release/serabut -i eth0 --data-dir /srv/pxe --os ubuntu-24.04
```

### Example 10: Verbose Debugging

```bash
sudo ./target/release/serabut -i eth0 -v
```

### Example 11: Different HTTP Port for Autoinstall

If port 8080 is in use:

```bash
sudo ./target/release/serabut -i eth0 --autoinstall --http-port 3000
```

## User-Data Examples

### Development Server

```yaml
#cloud-config
autoinstall:
  version: 1
  locale: en_US.UTF-8
  keyboard:
    layout: us
  identity:
    hostname: dev-server
    username: developer
    password: "$6$xyz$hash..."
  ssh:
    install-server: true
    allow-pw: true
  packages:
    - build-essential
    - git
    - docker.io
    - vim
  storage:
    layout:
      name: direct
  late-commands:
    - curtin in-target -- systemctl enable docker
    - curtin in-target -- usermod -aG docker developer
```

### Minimal Server (Wipe Disk, SSH Only)

```yaml
#cloud-config
autoinstall:
  version: 1
  identity:
    hostname: minimal
    username: sysadmin
    password: "$6$xyz$hash..."
  ssh:
    install-server: true
    allow-pw: false
    authorized-keys:
      - ssh-ed25519 AAAAC3... your-public-key
  storage:
    layout:
      name: lvm
```

### Static IP Configuration

```yaml
#cloud-config
autoinstall:
  version: 1
  network:
    network:
      version: 2
      ethernets:
        eth0:
          addresses:
            - 192.168.1.100/24
          gateway4: 192.168.1.1
          nameservers:
            addresses:
              - 8.8.8.8
              - 8.8.4.4
  identity:
    hostname: static-server
    username: admin
    password: "$6$xyz$hash..."
  ssh:
    install-server: true
```

## Generating Password Hashes

For the `password` field in user-data, generate a SHA-512 hash:

```bash
# Using mkpasswd (from whois package)
mkpasswd -m sha-512 "your-password"

# Using Python
python3 -c "import crypt; print(crypt.crypt('your-password', crypt.mksalt(crypt.METHOD_SHA512)))"

# Using openssl
openssl passwd -6 "your-password"
```

## Command Line Options

| Option | Description |
|--------|-------------|
| `-i, --interface <NAME>` | Network interface (required for server mode, derives IP automatically) |
| `--os <ID>` | Operating system to serve (default: ubuntu-24.04) |
| `--data-dir <PATH>` | Directory for netboot files (default: /var/lib/serabut) |
| `--tftp-port <PORT>` | TFTP server port (default: 69) |
| `--skip-download` | Skip netboot download, use existing files |
| `--monitor-only` | Monitor only mode, no TFTP/proxyDHCP servers |
| `--autoinstall` | Enable Ubuntu autoinstall with cloud-init HTTP server |
| `--user-data <PATH>` | Path to user-data file for autoinstall |
| `--http-port <PORT>` | Cloud-init HTTP server port (default: 8080) |
| `-v, --verbose` | Enable verbose output |
| `--no-color` | Disable colored output |
| `--list-interfaces` | List available network interfaces and exit |
| `--list-os` | List available operating systems and exit |
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
| 8080 | TCP | Cloud-init HTTP (for autoinstall) |

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
