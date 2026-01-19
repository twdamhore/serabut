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
cd /var/lib/serabutd/iso
mkdir ubuntu-24.04
cd ubuntu-24.04

# Create iso.cfg pointing to your ISO file
echo "filename=ubuntu-24.04-live-server-amd64.iso" > iso.cfg

# Copy or symlink the ISO
cp /path/to/ubuntu-24.04-live-server-amd64.iso .

# Create boot script template
cat > boot.ipxe.j2 << 'EOF'
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp autoinstall ds=nocloud-net;s=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
imgfetch http://{{ host }}:{{ port }}/action/remove?mac={{ mac }} ||
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
mkdir -p /var/lib/serabutd/hardware/aa-bb-cc-dd-ee-ff
echo "hostname=server01" > /var/lib/serabutd/hardware/aa-bb-cc-dd-ee-ff/hardware.cfg
```

### 3. Schedule Installation

```bash
# Add MAC to action.cfg: mac=iso,automation-profile
echo "aa-bb-cc-dd-ee-ff=ubuntu-24.04,default" >> /var/lib/serabutd/action.cfg
```

### 4. Boot the Machine

PXE boot the machine. After installation completes, the MAC is commented out in `action.cfg` and subsequent boots go to local disk.

To reinstall: uncomment or re-add the MAC line in `action.cfg`.

## Directory Structure

```
/var/lib/serabutd/
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

## File Reference

### action.cfg (Required)

Maps MAC addresses to ISO and automation profile for installation.

```ini
# Format: mac=iso-name,automation-profile
aa-bb-cc-dd-ee-ff=ubuntu-24.04,default
11-22-33-44-55-66=alma-9.4,webserver

# After installation, lines are commented out:
# completed aa-bb-cc-dd-ee-ff on 2026-01-18T12:00:00-UTC
# aa-bb-cc-dd-ee-ff=ubuntu-24.04,default
```

| Field | Required | Description |
|-------|----------|-------------|
| MAC address | Yes | Hyphen-separated format (aa-bb-cc-dd-ee-ff) |
| iso-name | Yes | Must match a directory under `iso/` |
| automation-profile | Yes | Must match a directory under `iso/{name}/automation/` |

---

### hardware.cfg (Required per machine)

Location: `/var/lib/serabutd/hardware/{mac}/hardware.cfg`

```ini
hostname=server01
realname=John Doe
username=admin
password=$6$rounds=4096$salt$hashedpassword
ssh_key_1=ssh-ed25519 AAAAC3... user@host
```

| Key | Required | Description |
|-----|----------|-------------|
| `hostname` | **Yes** | Machine hostname, used in templates |
| `realname` | No | User's display name |
| `username` | No | Login username |
| `password` | No | Hashed password (generate with `openssl passwd -6`) |
| `ssh_key_1` | No | SSH public key |
| (any other) | No | Custom variables available as `{{ key }}` in templates |

---

### iso.cfg (Required per ISO)

Location: `/var/lib/serabutd/iso/{iso-name}/iso.cfg`

```ini
filename=ubuntu-24.04-live-server-amd64.iso
```

| Key | Required | Description |
|-----|----------|-------------|
| `filename` | **Yes** | Name of the ISO file in the same directory |

---

### boot.ipxe.j2 (Required per ISO)

Location: `/var/lib/serabutd/iso/{iso-name}/boot.ipxe.j2`

iPXE script template returned by `/boot?mac={mac}`.

**Ubuntu autoinstall example:**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp autoinstall ds=nocloud-net;s=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
imgfetch http://{{ host }}:{{ port }}/action/remove?mac={{ mac }} ||
boot
```

**Ubuntu manual installation example:**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp url=http://{{ host }}:{{ port }}/iso/{{ iso }}/{{ iso_image }} cloud-config-url=/dev/null
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
imgfetch http://{{ host }}:{{ port }}/action/remove?mac={{ mac }} ||
boot
```

**AlmaLinux/RHEL example:**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/images/pxeboot/vmlinuz ip=dhcp inst.ks=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/kickstart.ks
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/images/pxeboot/initrd.img
imgfetch http://{{ host }}:{{ port }}/action/remove?mac={{ mac }} ||
boot
```

---

### Automation Templates (Required per profile)

Location: `/var/lib/serabutd/iso/{iso-name}/automation/{profile}/`

**Ubuntu (cloud-init) - user-data.j2:**
```yaml
#cloud-config
autoinstall:
  version: 1
  locale: en_US.UTF-8
  keyboard:
    layout: us
    variant: ""
  source:
    id: ubuntu-server
    search_drivers: false
  network:
    version: 2
    ethernets:
      id0:
        match:
          driver: "*"
        dhcp4: true
  proxy: ""
  storage:
    layout:
      name: direct
  identity:
    realname: {{ realname }}
    hostname: {{ hostname }}
    username: {{ username }}
    password: {{ password }}
  ssh:
    install-server: true
    authorized-keys:
      - {{ ssh_key_1 }}
    allow-pw: false
  snaps: []
  updates: all
  shutdown: reboot
```

**Ubuntu - meta-data.j2:** (can be empty)
```yaml
instance-id: {{ hostname }}
local-hostname: {{ hostname }}
```

**AlmaLinux/RHEL - kickstart.ks.j2:**
```
#version=RHEL9
text
network --bootproto=dhcp --hostname={{ hostname }}
rootpw --iscrypted $6$rounds=4096$...
keyboard --vckeymap=us
lang en_US.UTF-8
timezone UTC
bootloader --location=mbr
clearpart --all --initlabel
autopart --type=lvm
reboot

%packages
@^minimal-environment
%end
```

---

## Template Variables

Available in all `.j2` templates:

| Variable | Source | Description |
|----------|--------|-------------|
| `{{ host }}` | Request header | Server hostname/IP |
| `{{ port }}` | Request header | Server port |
| `{{ mac }}` | Request query | Client MAC address |
| `{{ iso }}` | action.cfg | ISO name (directory name) |
| `{{ iso_image }}` | iso.cfg | ISO filename from `filename=` |
| `{{ automation }}` | action.cfg | Automation profile name |
| `{{ hostname }}` | hardware.cfg | Machine hostname (**required**) |
| `{{ realname }}` | hardware.cfg | User's display name |
| `{{ username }}` | hardware.cfg | Login username |
| `{{ password }}` | hardware.cfg | Hashed password (generate with `openssl passwd -6`) |
| `{{ ssh_key_1 }}` | hardware.cfg | SSH public key for authorized_keys |
| `{{ <key> }}` | hardware.cfg | Any custom key from hardware.cfg |

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
