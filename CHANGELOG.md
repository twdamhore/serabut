# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add tar.gz/tgz archive support for serving files (e.g., netboot archives)

## [0.2.0] - 2026-01-19

### Added

- Add `b64decode` and `b64encode` template filters for base64 encoding/decoding in templates

### Removed

- Remove `network_interface` field from hardware configuration

## [0.1.0] - 2026-01-18

### Added

- Initial release of Serabut PXE boot server
- HTTP server for PXE booting multiple operating systems
- Support for automated OS installations via iPXE
- ISO file serving directly from ISO images without mounting
- Template rendering with Jinja2-style syntax (minijinja)
- Hardware configuration per MAC address
  - Hostname configuration (mandatory)
  - Timezone configuration (optional)
  - Machine ID configuration (optional)
  - SSH host keys (ECDSA, Ed25519, RSA - optional)
- Action configuration for MAC-to-ISO mapping
- Profile-based boot template overrides
- Duplicate MAC entry detection with warning
- HTTP request logging with client IP
- Content-Length header for ISO downloads
- Configuration hot-reload via SIGHUP
- Graceful shutdown handling (SIGTERM, SIGINT)
- Systemd service file with security hardening
- Logrotate configuration
- Comprehensive documentation in README

### Security

- Systemd hardening (ProtectSystem, ProtectHome, NoNewPrivileges)
- File locking for concurrent action.cfg access
- Input validation for MAC addresses

[Unreleased]: https://github.com/twdamhore/serabut/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/twdamhore/serabut/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/twdamhore/serabut/releases/tag/v0.1.0
