# Serabut

HTTP server for PXE booting multiple OSes with automated installation.

Serabut serves boot scripts and files directly from ISO images without mounting them. Combined with iPXE and DHCP/TFTP (e.g., dnsmasq), it enables fully automated OS installations.

## Motivation
I tried a few other PXE boot managers and they did not meet my requirements.
I wanted:
- Ability to re-install using official ISO image. Some solutions were using cloud images. These are still official, just that they are slightly different from installing it from ISO images.
- Support for boxes without BMC (Baseboard Management Controller). I am too lazy to physically press a button.
- Some implementations were tied to specific OS or OS-release. For example, it works with ubuntu 22.04 but not 24.04.
- I am also too lazy to extract the kernel from the ISO to boot up the installation process.
- Using `dnsmasq` and `nginx` would cover most of the things I needed but I need to manually turn on `dnsmasq` otherwise there was no state and the next reboot would re-install the ISO on the target box, again, and again.

## What it does
- ✅ I do not have to extract `vmlinuz`/`initrd`. The files are read from the `.iso` file.
- ✅ I do not need a computer with BMC. My mini PC works. One PXE boot then `serabut` will take it from there and the next boot is from a clean installation. No physical button to press.
- ✅ I do not tight coupling with the current OS/distro/release. The running `ldd` on the one binary, I can see 4 libraries: `linux-vdso`/`libgcc_s`/`libm`/`libc`.
- ✅ Tested on Ubuntu 22.04/24.04/25.10.
- ✅ Supports Debian netboot with automatic firmware injection (initrd + firmware.cpio.gz concatenation).
- ❓ To be tested on AlmaLinux and Rocky Linux.

## Leverage
I settle for using `dnsmasq` for for Proxy DHCP and the initial TFTP transfer. All other file transfer was on http and `serabut` would serve the content. There was no need to leverage on `nginx`. I would configure the MAC address and what to install on a file, and then when it is done, the final step of the installation process would call the URL endpoint that would remove the entry. The next boot would not re-start the installation process again.

## How It Works

```
┌─────────────┐    DHCP/TFTP     ┌─────────────┐    HTTP      ┌─────────────┐
│   Machine   │ ───────────────► │   dnsmasq   │ ──────────►  │   serabut   │
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
| `GET /done/{mac}` | Marks MAC as installed (prevents reinstall loop) |

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
imgfetch http://{{ host }}:{{ port }}/done/{{ mac }} ||
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
timezone=America/New_York
machine_id=srv-001
role=webserver
```

| Key | Required | Description |
|-----|----------|-------------|
| `hostname` | **Yes** | Machine hostname |
| `timezone` | No | Timezone (e.g., `America/New_York`, `UTC`) |
| `machine_id` | No | Machine identifier for tracking |
| `base64_ssh_host_key_ecdsa_public` | No | Base64-encoded ECDSA public host key |
| `base64_ssh_host_key_ecdsa_private` | No | Base64-encoded ECDSA private host key |
| `base64_ssh_host_key_ed25519_public` | No | Base64-encoded Ed25519 public host key |
| `base64_ssh_host_key_ed25519_private` | No | Base64-encoded Ed25519 private host key |
| `base64_ssh_host_key_rsa_public` | No | Base64-encoded RSA public host key |
| `base64_ssh_host_key_rsa_private` | No | Base64-encoded RSA private host key |
| (any other) | No | Custom variables available as `{{ key }}` in templates |

---

### iso.cfg (Required per ISO)

Location: `/var/lib/serabutd/iso/{iso-name}/iso.cfg`

```ini
filename=ubuntu-24.04-live-server-amd64.iso
```

**Debian with firmware injection:**
```ini
filename=debian-13.3.0-amd64-netinst.iso
initrd_path=/install.amd/initrd.gz
firmware=firmware.cpio.gz
```

| Key | Required | Description |
|-----|----------|-------------|
| `filename` | **Yes** | Name of the ISO file in the same directory |
| `initrd_path` | No | Path to initrd inside ISO (for firmware concatenation) |
| `firmware` | No | Firmware file to append to initrd (e.g., `firmware.cpio.gz`) |

When both `initrd_path` and `firmware` are set, requests for the initrd path will automatically serve the initrd with firmware appended. This is required for Debian netboot installations that need non-free firmware. See [Debian NetbootFirmware wiki](https://wiki.debian.org/DebianInstaller/NetbootFirmware).

---

### boot.ipxe.j2 (Required per ISO)

Location: `/var/lib/serabutd/iso/{iso-name}/boot.ipxe.j2`

iPXE script template returned by `/boot?mac={mac}`.

**Ubuntu autoinstall example:**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp autoinstall ds=nocloud-net;s=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
imgfetch http://{{ host }}:{{ port }}/done/{{ mac }} ||
boot
```

**Ubuntu manual installation example:**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp url=http://{{ host }}:{{ port }}/iso/{{ iso }}/{{ iso_image }} cloud-config-url=/dev/null
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
imgfetch http://{{ host }}:{{ port }}/done/{{ mac }} ||
boot
```

**AlmaLinux/RHEL example:**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/images/pxeboot/vmlinuz ip=dhcp inst.ks=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/kickstart.ks
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/images/pxeboot/initrd.img
imgfetch http://{{ host }}:{{ port }}/done/{{ mac }} ||
boot
```

**Debian example (with firmware):**
```ipxe
#!ipxe
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/install.amd/vmlinuz auto=true priority=critical url=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/{{ mac }}/preseed.cfg
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/install.amd/initrd.gz
imgfetch http://{{ host }}:{{ port }}/done/{{ mac }} ||
boot
```

Note: When `initrd_path` and `firmware` are configured in `iso.cfg`, the initrd request automatically includes the firmware ([why?](https://wiki.debian.org/DebianInstaller/NetbootFirmware)). No special URL needed.

---

### Automation Templates (Required per profile)

Location: `/var/lib/serabutd/iso/{iso-name}/automation/{profile}/`

**Ubuntu (cloud-init) - user-data.j2:**
```yaml
#cloud-config
autoinstall:
  version: 1
  identity:
    hostname: {{ hostname }}
    username: admin
    password: $6$rounds=4096$...  # hashed password
  ssh:
    install-server: true
    allow-pw: false
  late-commands:
    - curtin in-target -- systemctl enable ssh
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

**Debian - preseed.cfg.j2:**
```
d-i debian-installer/locale string en_US.UTF-8
d-i keyboard-configuration/xkb-keymap select us
d-i netcfg/choose_interface select auto
d-i netcfg/get_hostname string {{ hostname }}
d-i mirror/country string manual
d-i mirror/http/hostname string deb.debian.org
d-i mirror/http/directory string /debian
d-i passwd/root-password-crypted password $6$rounds=4096$...
d-i clock-setup/utc boolean true
d-i time/zone string UTC
d-i partman-auto/method string lvm
d-i partman-auto/choose_recipe select atomic
d-i partman/confirm boolean true
d-i partman/confirm_nooverwrite boolean true
tasksel tasksel/first multiselect standard, ssh-server
d-i finish-install/reboot_in_progress note
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
| `{{ timezone }}` | hardware.cfg | Timezone (if set) |
| `{{ machine_id }}` | hardware.cfg | Machine identifier (if set) |
| `{{ base64_ssh_host_key_*_public }}` | hardware.cfg | Base64 SSH public host keys (if set) |
| `{{ base64_ssh_host_key_*_private }}` | hardware.cfg | Base64 SSH private host keys (if set) |
| `{{ <key> }}` | hardware.cfg | Any custom key from hardware.cfg |

### Template Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `b64decode` | Decode base64 string to UTF-8 | `{{ base64_ssh_host_key_ed25519_public \| b64decode }}` |
| `b64encode` | Encode string to base64 | `{{ hostname \| b64encode }}` |

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
