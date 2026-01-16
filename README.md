## serabutd

PXE ProxyDHCP daemon (UDP 4011) that responds to PXE clients with `next-server`
and an iPXE boot filename based on client architecture.

## Build

```bash
cargo build --release
```

Binary will be at `target/release/pxe-proxy`.

## Run

```bash
sudo ./target/release/pxe-proxy -i <iface>
```

The daemon resolves the IPv4 address on the specified interface and uses it as
`next-server`/TFTP server.

## Systemd

Use `deploy/pxe-proxy.service` as a template:

```bash
sudo install -m 0755 target/release/pxe-proxy /usr/local/bin/pxe-proxy
sudo install -m 0644 deploy/pxe-proxy.service /etc/systemd/system/pxe-proxy.service
sudo systemctl daemon-reload
sudo systemctl enable --now pxe-proxy.service
```

Edit the service file to set the correct interface (replace `eth0`).

## TFTP Root

Minimum files for iPXE chainloading:

- `undionly.kpxe` (BIOS)
- `ipxe.efi` (UEFI x64)
