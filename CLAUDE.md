# Claude Context for Serabut

## Project Overview
Serabut is a Rust HTTP server for PXE booting. It serves boot scripts and files directly from ISO images without mounting them. Used with iPXE and dnsmasq for automated OS installations.

## Tech Stack
- Rust with Axum web framework
- Tokio async runtime
- iso9660-rs for reading ISO files
- Minijinja for templating
- Runs on port 4123 by default

## Key Directories
```
src/
├── main.rs           # Entry point, server setup
├── config.rs         # AppState, config parsing
├── error.rs          # Error types (AppError)
├── utils.rs          # MAC normalization, host parsing
├── routes/
│   ├── boot.rs       # GET /boot?mac={mac}
│   ├── iso.rs        # GET /iso/{iso}/{path}
│   └── action.rs     # GET /done/{mac}
└── services/
    ├── iso.rs        # ISO reading, streaming, firmware concat
    ├── template.rs   # Jinja template rendering
    ├── action.rs     # action.cfg management
    └── hardware.rs   # hardware.cfg loading
```

## Recent Work (as of 2026-01-20)

### PR #101 - ISO Streaming (merged)
Implemented chunked streaming for ISO file reads to reduce memory usage:
- Changed from loading entire files into memory to 32MB chunked streaming
- Uses `tokio::sync::mpsc` channel with capacity 2 (backpressure)
- Max ~96MB in memory (3 chunks: 2 in channel + 1 being read)
- `spawn_blocking` for synchronous ISO reads
- `stream_from_iso()` and `stream_initrd_with_firmware()` methods
- Added `tokio-stream` and `bytes` dependencies

### PR #100 - cargo-deny fixes (merged)
- Fixed deprecated keys in deny.toml
- Added Unicode-3.0 license
- Committed Cargo.lock for security audits

## Build & Test
```bash
cargo build
cargo test          # 62 tests
cargo clippy
cargo deny check    # License/security audit
```

## CI Checks
- Dependency Check (cargo deny)
- Security Audit

## Useful Commands
```bash
# Watch memory during testing
watch -n1 'ps -o rss= -p $(pgrep serabutd)'

# Test ISO file streaming
curl http://localhost:4123/iso/ubuntu-24.04/casper/initrd -o /dev/null
```

## Notes
- All responses include Content-Length header (calculated upfront)
- FileBlockIo is not Send, so ISO must be reopened in spawn_blocking tasks
- MAC addresses use hyphen format: aa-bb-cc-dd-ee-ff
- Templates use .j2 extension and Jinja syntax
