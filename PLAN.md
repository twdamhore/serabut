# Serabut - PXE Boot Server

## Overview

HTTP server for PXE booting multiple OSes with multiple configurations.

## Endpoints

```
GET /boot?mac={mac}
    → Looks up MAC in action.cfg
    → If found: renders {iso}/boot.ipxe.j2
    → If not found: 404 (falls through to local boot)

GET /iso/{iso-name}/{path}
    → If {path} matches filename in iso.cfg → serve whole ISO file
    → If {path}.j2 exists in config dir → render template
    → Else → stream from ISO via iso9660_simple

GET /action/remove?mac={mac}
    → Comments out MAC line in action.cfg
    → Adds: # completed {mac} on {timestamp}-UTC
```

## Directory Structure

```
/var/lib/serabutd/config/
  action.cfg

  hardware/
    10-10-30-40-50-60/
      hardware.cfg              → hostname=server01

  iso/
    ubuntu-24.04.3/
      iso.cfg                   → filename=ubuntu-24.04.3-live-server-amd64.iso
      boot.ipxe.j2
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
        minimal/
          kickstart.ks.j2
        webserver/
          kickstart.ks.j2
```

## Config Files

### action.cfg

```
10-10-30-40-50-60=ubuntu-24.04.3,docker
aa-bb-cc-dd-ee-ff=alma-9.4,webserver
```

After boot:
```
# completed 10-10-30-40-50-60 on 2026-01-18T09:00:00-UTC
# 10-10-30-40-50-60=ubuntu-24.04.3,docker
aa-bb-cc-dd-ee-ff=alma-9.4,webserver
```

### hardware.cfg

```
hostname=server01
```

### iso.cfg

```
filename=ubuntu-24.04.3-live-server-amd64.iso
```

## TFTP Bootstrap (served by dnsmasq)

```ipxe
#!ipxe
chain http://{{ host }}:{{ port }}/boot?mac=${mac:hexhyp} || sanboot --no-describe --drive 0x80
```

## Example boot.ipxe.j2 (Ubuntu)

```ipxe
#!ipxe
imgfetch http://{{ host }}:{{ port }}/action/remove?mac={{ mac }} ||
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz ip=dhcp autoinstall ds=nocloud-net;s=http://{{ host }}:{{ port }}/iso/{{ iso }}/automation/{{ automation }}/{{ mac }}/
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
boot
```

## Template Variables

Available in all templates:
- `{{ host }}` - from request Host header
- `{{ port }}` - from request Host header (or config default)
- `{{ mac }}` - client MAC address
- `{{ iso }}` - ISO name from action.cfg
- `{{ automation }}` - automation name from action.cfg
- `{{ hostname }}` - from hardware.cfg (required, 500 if missing)

## Flows

### Flow 1: MAC not in action.cfg (normal boot)

```
1. Machine PXE boots
2. DHCP (dnsmasq) → TFTP server address
3. TFTP serves bootstrap.ipxe
4. iPXE calls GET /boot?mac=10-10-30-40-50-60
5. Server checks action.cfg → not found → 404
6. iPXE chain fails → sanboot fallback
7. Machine boots from local disk
```

### Flow 2: MAC in action.cfg (install)

```
1. Machine PXE boots
2. DHCP (dnsmasq) → TFTP server address
3. TFTP serves bootstrap.ipxe
4. iPXE calls GET /boot?mac=10-10-30-40-50-60
5. Server finds: 10-10-30-40-50-60=ubuntu-24.04.3,docker
6. Server renders ubuntu-24.04.3/boot.ipxe.j2
7. Returns rendered iPXE script
8. iPXE fetches /action/remove → MAC commented out
9. iPXE fetches kernel via iso9660_simple
10. iPXE fetches initrd via iso9660_simple
11. iPXE boots kernel
12. Installer fetches /iso/ubuntu-24.04.3/automation/docker/10-10-30-40-50-60/user-data
13. Server renders user-data.j2 with hostname from hardware.cfg
14. Installer runs
15. Machine reboots → Flow 1 (MAC now commented out)
```

### Flow 3: Re-arm (reinstall)

```
1. Admin edits action.cfg:
   - Delete "# completed..." line
   - Uncomment config line (or add new line)
2. Reboot machine
3. Flow 2 takes over
```

## Configuration

### /etc/serabutd.conf

```
interface=0.0.0.0
port=4123
log_level=info
```

Defaults: all interfaces, port 4123, info logging

Log levels:
- `error` - only errors
- `warn` - errors + warnings
- `info` - errors + warnings + requests (including 404s)
- `debug` - everything (template rendering, config parsing, etc.)

## Installation

```
make            # build
sudo make install   # install binary + systemd service + default config
sudo make uninstall # remove
```

Installs:
- `/usr/local/bin/serabutd`
- `/etc/systemd/system/serabutd.service`
- `/etc/serabutd.conf` (if not exists)
- `/var/lib/serabutd/config/` (directory structure)

## Logging

Logs to stdout (captured by journald when running as service).

```
journalctl -u serabutd -f
```

Log entries:
- Startup: interface, port
- Each request: method, path, mac (if present), response status
- Action remove: mac, timestamp
- Errors: template rendering, ISO read failures, file not found

## Error Handling

- Missing hardware.cfg for MAC → 500 error, log error
- Missing iso.cfg → 500 error
- Template rendering failure → 500 error
- ISO file not found → 404
- File inside ISO not found → 404

## File Locking

action.cfg uses file locking for concurrent `/action/remove` requests.
Multiple machines booting simultaneously won't corrupt the file.

## Reload

```
sudo systemctl reload serabutd
```

SIGHUP reloads configuration (serabutd.conf) without restart.
Does not affect in-flight requests.

## Code Structure

```
src/
  main.rs           → entry point, signal handling, server startup
  config.rs         → serabutd.conf parsing, reload logic
  routes/
    mod.rs          → router setup
    boot.rs         → GET /boot handler
    iso.rs          → GET /iso handler
    action.rs       → GET /action/remove handler
  services/
    mod.rs
    action.rs       → action.cfg read/write with file locking
    hardware.rs     → hardware.cfg parsing
    iso.rs          → iso.cfg parsing, iso9660_simple reading
    template.rs     → MiniJinja rendering
  error.rs          → error types, conversions
```

## Code Quality

**SOLID Principles:**
- Single Responsibility: each module handles one concern
- Open/Closed: traits for testability (e.g., file system abstraction)
- Liskov Substitution: consistent error handling across modules
- Interface Segregation: small, focused traits
- Dependency Inversion: handlers depend on abstractions, not concrete implementations

**Guidelines:**
- Files: < 300 lines preferred, split if larger
- Functions: < 50 lines preferred, extract helpers if larger
- No functions < 3 lines unless trivially clear
- All public functions documented
- Error messages include context (file path, MAC address, etc.)

**Testing:**
- Unit tests in each module (`#[cfg(test)]`)
- Integration tests in `tests/`
- Mock file system for testing without real files
- Target: > 80% code coverage

```
make test           # run tests
make coverage       # run tests with coverage report
```

## Tech Stack

- Rust
- MiniJinja for templating
- iso9660_simple for reading ISO files without mounting
- axum for HTTP server
- tracing for logging
