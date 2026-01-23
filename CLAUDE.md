# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Serabut is a PXE boot infrastructure for Kubernetes. It provides network boot capabilities for automated OS installation (primarily Debian) with firmware injection and package caching.

## Architecture

**Kubernetes Deployment** - Single pod with 3 containers:

1. **dnsmasq** - proxyDHCP + TFTP server (serves ipxe.efi for UEFI boot)
2. **Go HTTP server** - Custom server for ISO serving, content extraction, firmware injection, boot configs, and preseed files
3. **Squid** - Caching proxy for apt package downloads

**Network** - Pod attaches directly to physical network (192.168.88.0/24) via Multus + Macvlan CNI, obtaining IP via DHCP. No cluster network.

**Storage** - Two NFS-backed PVCs:
- `iso` (10GB) - Stores bootable ISO files
- `squid` (10GB) - Squid cache storage

## Go HTTP Server Endpoints

| Path | Description |
|------|-------------|
| `/iso/download/{iso}` | Raw ISO file download |
| `/iso/content/{iso}/{path}` | Files extracted from inside ISO |
| `/with-firmware/{distro}/initrd.gz` | initrd + firmware.cpio.gz concatenated (see Debian NetbootFirmware) |
| `/boot/{distro}` | iPXE boot script for chainloading |
| `/configuration/{config}/automation/{distro}/preseed.cfg` | Preseed files for automated install |

## Build Commands

```bash
# Install Helm chart
helm install serabut ./charts/serabut

# Lint Helm chart
helm lint ./charts/serabut

# Template Helm chart (dry-run)
helm template serabut ./charts/serabut
```

## Key Conventions

- Node labeling: `serabut/interface=<interface-name>` identifies which network interface connects to the PXE network
- UEFI boot only (no legacy BIOS support)
- Debian 12 as primary target distro

## Future Work

- Custom Kubernetes controllers for managing boot configurations
- CRDs for defining distros, preseeds, and boot profiles
