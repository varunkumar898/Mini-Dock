# Changelog

All notable changes to `alphen-box` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-18

### Added - Milestone 2: Image Pull & Storage (Linux ARM64 & Windows ARM64)
- OCI Distribution & Registry client (`alphen-image::registry`):
  - Token authentication with Docker Hub (`auth.docker.io`), GitHub Container Registry (`ghcr.io`), and standard OCI registries.
  - Automatic multi-architecture manifest list resolution targeting `linux/arm64` (aarch64).
  - Pure-Rust TLS support using `ureq` and `rustls` with embedded webpki roots.
- Content-addressed blob store (`alphen-image::store`):
  - SHA-256 verified streaming downloads into `blobs/sha256/<hash>`.
  - Automatic layer deduplication across images.
  - Portable storage layout supporting both Linux (`/var/lib/alphen-box`) and Windows ARM (`./data/alphen-box`).
- Layer unpacking with OCI whiteout handling (`alphen-image::layer`):
  - Unpacks raw and gzip-compressed tar archives.
  - Full support for `.wh.<name>` explicit file whiteouts and `.wh..wh..opq` opaque directory whiteouts.
  - Path traversal security checks.
- CLI commands implemented:
  - `alphen-box pull <image>`
  - `alphen-box images`
  - `alphen-box inspect <image>`
  - `alphen-box rmi <image>`
  - `alphen-box gc`
- Comprehensive unit and integration test suite (`tests/image_lifecycle.rs`).
- Release binary size measured at **1.48 MB**, well under the 6 MB budget.

## [0.1.0] - 2026-11-18

### Added - Milestone 1: Skeleton
- Initial multi-crate Cargo workspace:
  - `alphen-common`: typed errors (`AlphenError`), result aliases, and minimal `env_logger` integration.
  - `alphen-core`: OCI image references, image summaries, data models, and serialization.
  - `alphen-image`: image store abstractions and directory layouts.
  - `alphen-runtime`: Linux kernel requirement checking (Linux 5.15+, cgroups v2, userns, overlayfs, seccomp).
  - `alphen-net`: network driver definitions (`none`, `host`, `bridge`) and port mapping parsers.
  - `alphen-box`: CLI binary implemented using `clap` (derive only, default features off).
- CLI commands:
  - `alphen-box version`: outputs runtime version, target architecture (`aarch64-unknown-linux-musl`), and host requirement verification status.
  - `alphen-box images`: lists cached container images in standard columnar format.
  - Flag declarations matching Docker syntax (`run`, `ps`, `exec`, `logs`, `stop`, `start`, `rm`, `inspect`, `rmi`, `gc`).
- Build & Automation:
  - `Makefile` with `build`, `build-arm64`, `test`, `fmt`, `clippy`, `size-check`, `bench`, `package`, and `clean` targets.
  - `scripts/size_check.py` to ensure release binaries stay strictly under the 2 MB M1 budget (achieved ~449 KB).
  - `scripts/package.py` to create the Alphen OS root staging directory and distribution tarball.
  - `packaging/APKBUILD` and `packaging/alphen-box.service` systemd unit.
  - GitHub Actions CI workflow for linting, host testing, and cross-compilation for `aarch64-unknown-linux-musl`.
