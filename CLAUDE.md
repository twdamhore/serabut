# Serabut v2 - PXE Boot Manager Redesign

## Overview

Serabut is a Rust HTTP server for PXE booting multiple OSes with automated installation. It serves boot scripts and files directly from ISO images without mounting them.

## Directory Structure

```
/var/lib/serabutd/
├── iso/                          # NFS-mountable, read-only, excludable from backups
│   ├── debian-12.8.0-amd64-netinst.iso
│   ├── debian-13.3.0-amd64-netinst.iso
│   ├── ubuntu-24.04-live-server-amd64.iso
│   ├── FreeBSD-14.0-RELEASE-amd64-disc1.iso
│   ├── debian-12/
│   │   └── firmware.cpio.gz      # Add-ons in subdirs
│   └── debian-13/
│       └── firmware.cpio.gz
├── views/                        # .j2 templates
│   ├── linux/
│   │   ├── debian/
│   │   │   ├── debian-12/
│   │   │   │   ├── boot.ipxe.j2
│   │   │   │   └── automation/
│   │   │   │       └── default/
│   │   │   │           └── preseed.cfg.j2
│   │   │   └── debian-13/
│   │   │       ├── boot.ipxe.j2
│   │   │       └── automation/
│   │   │           └── default/
│   │   │               └── preseed.cfg.j2
│   │   └── ubuntu/
│   │       └── ubuntu-24.04/
│   │           ├── boot.ipxe.j2
│   │           └── automation/
│   │               └── default/
│   │                   ├── user-data.j2
│   │                   └── meta-data.j2
│   └── bsd/
│       └── freebsd/
│           └── freebsd-14/
│               ├── boot.ipxe.j2
│               └── automation/
│                   └── default/
│                       └── installerconfig.j2
├── aliases.cfg                   # Release → ISO filename mapping
├── combine.cfg                   # Combined file definitions
├── action.cfg                    # Hostname → installation mapping
└── hardware/                     # Per-machine configs (hostname = filename without .cfg)
    ├── crane.cfg
    ├── falcon.cfg
    └── eagle.cfg
```

### Design Rationale

- **ISOs flat in `/iso/`**: Easy NFS mount, read-only, simple backup exclusion (`tar --exclude='*.iso'`)
- **Add-ons in subdirs**: Only when needed (e.g., Debian firmware per release)
- **Views separate**: Small .j2 files, easy to version control and backup
- **Hierarchy**: `views/{os}/{distro}/{release}/` supports Linux, BSD, and future OS families

## Configuration Files

### aliases.cfg

Maps release names to ISO filenames. Optional `downloadable` flag allows serving full ISO.

```ini
# Format: release=filename[,downloadable]

debian-12=debian-12.8.0-amd64-netinst.iso
debian-13=debian-13.3.0-amd64-netinst.iso
ubuntu-24.04=ubuntu-24.04-live-server-amd64.iso,downloadable
freebsd-14=FreeBSD-14.0-RELEASE-amd64-disc1.iso
```

### combine.cfg

Defines files that are concatenated on-the-fly. Used for Debian firmware injection.

```ini
# Format: name=content:{release}/{path},file:{relative_path}
# content: = read from inside ISO (via release alias)
# file: = read from /var/lib/serabutd/iso/

debian-12-initrd=content:debian-12/install.amd/initrd.gz,file:debian-12/firmware.cpio.gz
debian-13-initrd=content:debian-13/install.amd/initrd.gz,file:debian-13/firmware.cpio.gz
```

### action.cfg

Maps hostnames to OS installation. Only machines listed here will PXE boot.

```ini
# Format: hostname=release[,automation-profile]
# automation-profile defaults to "default" if omitted

crane=ubuntu-24.04
falcon=debian-13
eagle=freebsd-14,custom
```

### hardware/{hostname}.cfg

Per-machine configuration. Filename determines hostname (e.g., `crane.cfg` → hostname is `crane`).

```ini
# Required
mac=aa-bb-cc-dd-ee-ff

# Optional - available as template variables
timezone=UTC
disk=/dev/sda
# Any custom key=value pairs
```

## HTTP Endpoints

### Summary

| Endpoint | Description |
|----------|-------------|
| `GET /content/iso/{alias}/{path}` | Serve file from inside ISO |
| `GET /content/combine/{name}` | Serve concatenated file |
| `GET /content/raw/{alias}/{filename}` | Serve full ISO (if downloadable) |
| `GET /views/{path}?hostname={hostname}` | Render .j2 template |
| `GET /action/boot/{mac}` | Get iPXE boot script (looks up hostname by MAC) |
| `GET /action/done/{mac}` | Mark installation complete |

### Serve file from ISO

```
GET /content/iso/{alias}/{path}
```

Examples:
- `/content/iso/debian-13/install.amd/vmlinuz`
- `/content/iso/ubuntu-24.04/casper/vmlinuz`
- `/content/iso/ubuntu-24.04/casper/initrd`

Flow:
1. Look up alias in `aliases.cfg`
2. Read path from inside ISO
3. Stream to client

### Serve combined file

```
GET /content/combine/{name}
```

Examples:
- `/content/combine/debian-13-initrd`

Flow:
1. Look up name in `combine.cfg`
2. Parse sources (content:// and file://)
3. Concatenate and stream to client

### Serve full ISO (downloadable only)

```
GET /content/raw/{alias}/{filename}
```

Examples:
- `/content/raw/ubuntu-24.04/ubuntu-24.04-live-server-amd64.iso`

Flow:
1. Check alias has `downloadable` flag
2. Validate filename matches aliases.cfg entry
3. Stream full ISO

Returns 403 if alias not marked downloadable.

### Render template

```
GET /views/{path}?hostname={hostname}
```

Examples:
- `/views/linux/ubuntu/ubuntu-24.04/boot.ipxe.j2?hostname=crane`
- `/views/linux/ubuntu/ubuntu-24.04/automation/default/user-data.j2?hostname=crane`

Flow:
1. Read .j2 file from `/var/lib/serabutd/views/{path}`
2. Load variables from `hardware/{hostname}.cfg`
3. Render template with variables (hostname implicit from filename)
4. Return rendered content

### Boot script

```
GET /action/boot/{mac}
```

Flow:
1. Scan `hardware/*.cfg` files to find one with matching `mac=` value
2. Extract hostname from filename (e.g., `crane.cfg` → `crane`)
3. Look up hostname in `action.cfg`
4. If found, return iPXE boot script; if not, return 404 (boot locally)

### Mark complete

```
GET /action/done/{mac}
```

Flow:
1. Find hostname by MAC (same as boot endpoint)
2. Comment out hostname entry in action.cfg

## Template Variables

Available in `.j2` templates:

| Variable | Source | Description |
|----------|--------|-------------|
| `host` | Request | Server hostname/IP |
| `port` | Request | Server port |
| `hostname` | Filename | Machine hostname (from {hostname}.cfg filename) |
| `mac` | hardware.cfg | Client MAC address |
| `os` | Derived | OS family (linux, bsd) - derived from release |
| `distro` | Derived | Distribution (debian, ubuntu, freebsd) - derived from release |
| `release` | action.cfg | Release (debian-13, ubuntu-24.04, freebsd-14) |
| `automation` | action.cfg | Automation profile name (default: "default") |
| `*` | hardware.cfg | Any custom key (timezone, disk, etc.) |

## Implementation Notes

- Written in Rust
- **Use jemalloc allocator** (better performance for long-running servers)
- Reads ISO files without mounting (iso9660 crate or similar)
- **Streaming in 1MB blocks** — never load full ISO into memory
- Single binary, minimal dependencies
- Systemd service: serabutd

### Content-Length Handling

Always send `Content-Length` header:

| Endpoint | Strategy |
|----------|----------|
| `/content/iso/...` | Read file size from ISO metadata before streaming |
| `/content/combine/...` | Sum sizes of all components before streaming |
| `/content/raw/...` | Read file size from filesystem |
| `/views/...` | Render template to memory buffer, get length, then send |

### Streaming Strategy

```
ISO/Raw files:
1. Get file size → set Content-Length
2. Stream in 1MB blocks
3. Never buffer entire file

Combined files:
1. Resolve all sources
2. Sum sizes → set Content-Length  
3. Stream each source in 1MB blocks sequentially

Rendered templates:
1. Render to String/Vec<u8> in memory
2. Set Content-Length from buffer size
3. Send buffer (templates are small)
```

## TODO

- [ ] Define boot.ipxe.j2 templates for each OS
- [ ] Define automation templates (preseed, cloud-init, etc.)
- [ ] BSD support specifics
- [ ] Config reload without restart
- [ ] Metrics/observability

