# Serabut

HTTP server for PXE booting multiple OSes with automated installation.

Serabut serves boot scripts and files directly from ISO images without mounting them. Combined with iPXE and DHCP/TFTP (e.g., dnsmasq), it enables fully automated OS installations.

## How It Works

```
┌─────────────┐    DHCP/TFTP     ┌─────────────┐    HTTP      ┌─────────────┐
│   Machine   │ ───────────────► │   dnsmasq   │ ──────────► │   serabut   │
│  (PXE boot) │                  │  (bootstrap)│              │  (port 4123)│
└─────────────┘                  └─────────────┘              └─────────────┘
                                                                    │
                                                                    ▼
                                                              ┌───────────┐
                                                              │ ISO files │
                                                              │ Templates │
                                                              │ Configs   │
                                                              └───────────┘
```

1. Machine PXE boots, gets iPXE bootstrap from TFTP
2. iPXE requests boot script from Serabut (`/boot?mac=xx-xx-xx-xx-xx-xx`)
3. If MAC is in `action.cfg`, Serabut returns a boot script
4. iPXE fetches kernel/initrd directly from ISO via Serabut
5. Installer fetches automation files (cloud-init, kickstart) from Serabut
6. After install, MAC is marked complete - next boot falls through to local disk

## Installation

```bash
# Build
make

# Install (creates serabut user, installs service)
sudo make install

# Start
sudo systemctl enable --now serabutd
```

## Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /boot?mac={mac}` | Returns iPXE boot script for MAC, or 404 to boot locally |
| `GET /iso/{iso}/{path}` | Serves files from ISO or renders templates |
| `GET /action/remove?mac={mac}` | Marks MAC as installed (prevents reinstall loop) |

## Quick Setup

### 1. Add an ISO

```bash
cd /var/lib/serabutd/config/iso
mkdir ubuntu-24.04
cd ubuntu-24.04

# Create iso.cfg pointing to your ISO file
echo "filename=ubuntu-24.04-live-server-amd64.iso" > iso.cfg

# Copy or symlink the ISO
cp /path/to/ubuntu-24.04-live-server-amd64.iso .

# Create boot script template
cat > boot.ipxe.j2 << 'EOF'
#!ipxe
imgfetch http://{{ host }}:{{ port }}/action/remove?mac={{ mac }} ||
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp autoinstall ds=nocloud-net;s=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
boot
EOF

# Create automation templates
mkdir -p automation/default
cat > automation/default/user-data.j2 << 'EOF'
#cloud-config
hostname: {{ hostname }}
# ... your cloud-init config
EOF

touch automation/default/meta-data.j2
```

### 2. Register Hardware

```bash
# Create hardware config for each machine (use MAC with hyphens)
mkdir -p /var/lib/serabutd/config/hardware/aa-bb-cc-dd-ee-ff
echo "hostname=server01" > /var/lib/serabutd/config/hardware/aa-bb-cc-dd-ee-ff/hardware.cfg
```

### 3. Schedule Installation

```bash
# Add MAC to action.cfg: mac=iso,automation-profile
echo "aa-bb-cc-dd-ee-ff=ubuntu-24.04,default" >> /var/lib/serabutd/config/action.cfg
```

### 4. Boot the Machine

PXE boot the machine. After installation completes, the MAC is commented out in `action.cfg` and subsequent boots go to local disk.

To reinstall: uncomment or re-add the MAC line in `action.cfg`.

## Directory Structure

```
/var/lib/serabutd/config/
├── action.cfg                     # MAC → ISO,profile mappings
├── hardware/
│   └── {mac}/
│       └── hardware.cfg           # hostname=xxx
└── iso/
    └── {iso-name}/
        ├── iso.cfg                # filename=xxx.iso
        ├── boot.ipxe.j2           # iPXE boot script template
        ├── xxx.iso                # The actual ISO file
        └── automation/
            └── {profile}/
                ├── user-data.j2   # Cloud-init (Ubuntu)
                └── kickstart.ks.j2 # Kickstart (RHEL/Alma)
```

## Template Variables

Available in all `.j2` templates:

| Variable | Description |
|----------|-------------|
| `{{ host }}` | Server hostname/IP from request |
| `{{ port }}` | Server port |
| `{{ mac }}` | Client MAC address |
| `{{ iso }}` | ISO name from action.cfg |
| `{{ automation }}` | Automation profile from action.cfg |
| `{{ hostname }}` | Hostname from hardware.cfg |

## Configuration

`/etc/serabutd.conf`:

```ini
interface=0.0.0.0    # Listen address (default: all interfaces)
port=4123            # Listen port (default: 4123)
log_level=info       # error, warn, info, debug
```

Reload config without restart:
```bash
sudo systemctl reload serabutd
```

## Logs

```bash
# Follow logs
tail -f /var/log/serabut/serabutd.log

# Or via journald
journalctl -u serabutd -f
```

## DHCP/TFTP Setup (dnsmasq example)

```ini
# /etc/dnsmasq.conf
enable-tftp
tftp-root=/var/lib/tftpboot

# Chain to iPXE
dhcp-match=set:ipxe,175
dhcp-boot=tag:!ipxe,undionly.kpxe
dhcp-boot=tag:ipxe,http://192.168.1.10:4123/boot?mac=${mac:hexhyp}
```

## License

MIT
