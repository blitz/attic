# Changelog

All notable changes to Celler will be documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `CELLER_TOKEN` environment variable as an alternative to `celler login`, useful for CI and scripted use.
- `celler push` now prints the total NAR size.
- Build trace support, so Nix can substitute the outputs of content-addressed derivations. Requires Nix 2.35 or newer on the client. Older Nix and Lix use a different scheme and cannot substitute.

### Changed

- Binaries renamed: `attic` → `celler`, `atticd` → `cellerd`.
- NixOS module renamed: `services.atticd` → `services.cellerd`.
- Environment variables renamed: `ATTIC_SERVER_*` → `CELLER_SERVER_*`.
- Replaced C++ FFI to `libnixstore` with the pure-Rust `nix-daemon` crate.
- Updated [sea-orm](https://www.sea-ql.org/SeaORM/) to v2.x.

### Removed

- WASM build target.
- Static package builds.
- Docker containers.

### Fixed

- S3 storage: improved tolerance for transient errors.
- Improved error logging for `push` and `watch-store`.

[Unreleased]: https://github.com/blitz/celler/compare/12cbeca141f46e1ade76728bce8adc447f2166c6...HEAD
