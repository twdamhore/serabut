# Serabut

PXE boot infrastructure for Kubernetes. Deploys a network boot environment with DHCP proxy, TFTP, HTTP serving, and caching proxy.

## Architecture

Single pod with 3 containers:

| Container | Role | Storage |
|-----------|------|---------|
| dnsmasq | proxyDHCP + TFTP (serves ipxe.efi) | — |
| Go HTTP server | ISO/content serving, firmware injection, boot configs | `iso` PVC (10GB) |
| Squid | Caching proxy for apt downloads | `squid` PVC (10GB) |

### HTTP Server Endpoints

| Path | Description |
|------|-------------|
| `/iso/download/{iso}` | Raw ISO download |
| `/iso/content/{iso}/{path}` | Files extracted from inside ISO |
| `/with-firmware/{distro}/initrd.gz` | initrd + firmware.cpio.gz concatenated |
| `/boot/{distro}` | iPXE boot script |
| `/configuration/{config}/automation/{distro}/preseed.cfg` | Preseed for automated install |

### Network Flow

```
Client UEFI PXE
  → Existing DHCP (192.168.88.1) assigns IP
  → dnsmasq proxyDHCP: boot ipxe.efi via TFTP at <pod-ip>
  → iPXE fetches boot script from HTTP
  → HTTP serves kernel + initrd (with firmware)
  → Debian installer boots with preseed
  → apt uses Squid proxy → packages cached
```

## Prerequisites

### 1. Multus CNI + DHCP CNI Daemon

Required for attaching pod to physical network (192.168.88.0/24).

The DHCP CNI daemon must be running on each node for macvlan DHCP to work:

```bash
# The dhcp daemon is part of CNI plugins, typically at:
/opt/cni/bin/dhcp daemon &
```

Or run it as a DaemonSet. See [CNI DHCP plugin documentation](https://www.cni.dev/plugins/current/ipam/dhcp/).

### 2. Node Labeling

Label each node with the interface connected to 192.168.88.0/24:

```bash
kubectl label nodes <node-name> serabut/interface=<interface-name>
```

Example:
```bash
kubectl label nodes crane serabut/interface=enp1s0
kubectl label nodes worker1 serabut/interface=eth1
```

### 3. NFS Storage Classes

Install an NFS provisioner (e.g., `nfs-subdir-external-provisioner`) twice to create storage classes:

```bash
# Storage for Squid cache
helm install squid nfs-subdir-external-provisioner/nfs-subdir-external-provisioner \
  --set storageClass.name=squid \
  --set nfs.server=<nfs-server-ip> \
  --set nfs.path=/path/to/squid

# Storage for ISOs
helm install iso nfs-subdir-external-provisioner/nfs-subdir-external-provisioner \
  --set storageClass.name=iso \
  --set nfs.server=<nfs-server-ip> \
  --set nfs.path=/path/to/iso
```

### 4. Existing DHCP Server

A DHCP server must exist on 192.168.88.0/24 (e.g., at 192.168.88.1). Serabut runs in proxyDHCP mode and does not conflict.

## Installation

Complete all prerequisites above first, then:

```bash
git clone https://github.com/twdamhore/serabut
cd serabut
helm install serabut ./charts/serabut
```

## Roadmap

- [ ] Custom controllers for managing boot configurations
- [ ] CRDs for defining distros, preseeds, and boot profiles
- [ ] Multi-distro support (Ubuntu, Fedora, etc.)

## Components

- **Alpine Linux** (base image)
- **dnsmasq** (proxyDHCP + TFTP)
- **iPXE** (UEFI network boot)
- **Squid** (caching proxy)
- **Custom Go HTTP server** (ISO serving, firmware injection)
