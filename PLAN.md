# Serabut - PXE Boot Server

## Overview

HTTP server for PXE booting multiple OSes with multiple configurations.

**Flow:**
1. TFTP (dnsmasq) serves initial iPXE script
2. iPXE chains to `http://{host}/boot?mac={mac}`
3. Server returns boot config based on MAC's pending action
4. Before boot, iPXE calls `/action/remove?mac={mac}` (one-shot install)
5. Machine boots, installs, reboots to local disk

## Endpoints

```
GET  /boot?mac={mac}
     → Rendered iPXE script for this MAC
     → Looks up action.cfg → iso + automation
     → Renders {iso}/boot.ipxe.j2

GET  /iso/{iso-name}/{path}
     → Three behaviors:
       1. {path} matches filename in iso.cfg → serve whole ISO
       2. {path}.j2 exists in config dir → render template
       3. else → stream from ISO via cdfs

GET  /action/remove?mac={mac}
     → Remove MAC entry from action.cfg

POST /action/add?mac={mac}&iso={iso}&automation={name}
     → Add MAC to action.cfg
```

## Directory Structure

```
/var/lib/serabut/
  config/
    action.cfg                    → pending installs

    hardware/
      10-10-30-40-50-60/
        hardware.cfg              → hostname=server01

    iso/
      ubuntu-24.04.3/
        iso.cfg                   → filename=ubuntu-24.04.3-live-server-amd64.iso
        boot.ipxe.j2              → iPXE boot template
        ubuntu-24.04.3-live-server-amd64.iso
        automation/
          minimal/
            user-data.j2
            meta-data.j2
          docker/
            user-data.j2
            meta-data.j2

      alma-9.4/
        iso.cfg                   → filename=AlmaLinux-9.4-x86_64-dvd.iso
        boot.ipxe.j2
        AlmaLinux-9.4-x86_64-dvd.iso
        automation/
          minimal.ks.j2
          webserver.ks.j2
```

## Config Files

### action.cfg
```ini
[10-10-30-40-50-60]
iso=ubuntu-24.04.3
automation=docker

[aa-bb-cc-dd-ee-ff]
iso=alma-9.4
automation=webserver
```

### hardware.cfg
```ini
hostname=server01
```

### iso.cfg
```ini
filename=ubuntu-24.04.3-live-server-amd64.iso
```

## TFTP Bootstrap (served by dnsmasq)

```ipxe
#!ipxe
chain http://{{ host }}:{{ port }}/boot?mac=${mac:hexhyp} || sanboot --no-describe --drive 0x80
```

## Template Variables

Available in all templates:
- `{{ host }}` - server hostname/IP
- `{{ port }}` - server port
- `{{ mac }}` - client MAC address
- `{{ hostname }}` - from hardware.cfg (if exists)

## Tech Stack

- Rust
- MiniJinja for templating
- cdfs for reading ISO files without mounting
- HTTP server (axum?)

## Open Questions

- Listing available ISOs / automations?
- Health check endpoint?
- Unknown MAC behavior (menu? 404?)
- Web UI for management?
