# Milestone 2: Image Pull, Storage, and Deduplication

## Overview
Milestone 2 delivers OCI and Docker v2 registry client integration, content-addressed layer blob storage, on-the-fly SHA-256 digest verification, cross-image layer deduplication, full OCI whiteout handling (`.wh.*` and `.wh..wh..opq`), local image metadata indexing, and garbage collection (`alphen-box gc`).

Support for both **Linux ARM64** (`aarch64-unknown-linux-musl`) on Alphen OS and **Windows ARM64** (`aarch64-pc-windows`) is fully implemented.

## Key Features Implemented

1. **OCI Distribution Registry Client (`alphen-image::registry`)**:
   - Token authentication via `Www-Authenticate` Bearer challenge against Docker Hub (`auth.docker.io`), GitHub Container Registry (`ghcr.io`), and standard OCI registries.
   - Automatic multi-architecture manifest list resolution: selects `linux/arm64` (or `aarch64` / `armv7` fallback) manifests automatically.
   - Blocking, low-overhead HTTP with `ureq` and pure `rustls` with `webpki-roots` (no Tokio or async runtime overhead).

2. **Content-Addressed Blob Storage (`alphen-image::store`)**:
   - Blobs stored by SHA-256 digest under `blobs/sha256/<hash>`.
   - Streaming hash calculation: SHA-256 digest is strictly verified on download. If a digest mismatch occurs, the file is aborted.
   - Automatic cross-image deduplication: if a layer already exists locally, download is skipped.

3. **Layer Unpacking & OCI Whiteout Support (`alphen-image::layer`)**:
   - Supports raw and gzip-compressed tar archives (`flate2` pure-rust backend).
   - OCI explicit whiteouts: files matching `.wh.<filename>` remove the corresponding file from lower layers.
   - OCI opaque whiteouts: directories containing `.wh..wh..opq` hide all existing lower-layer entries before writing upper entries.
   - Directory traversal protection: paths escaping the extraction root are rejected.

4. **CLI Commands**:
   - `alphen-box pull <image>`: Pulls image from registry, validates all blobs, stores manifest & metadata.
   - `alphen-box images`: Formatted table showing repository, tag, short ID, creation date, and total size.
   - `alphen-box inspect <image>`: Displays complete JSON metadata, layer digests, environment variables, and entrypoint.
   - `alphen-box rmi <image>`: Untags and removes image reference.
   - `alphen-box gc`: Collects all active references and deletes orphaned layer and config blobs, reclaiming disk space.

## Binary Size & Budgets

Hard budget target: **< 6.0 MB** release binary stripped.

```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    1,556,992 bytes (1520.5 KB / 1.48 MB)
 Budget (M2):      6,291,456 bytes (6.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (1.48 MB <= 6.0 MB)
```
- **Measured Size**: **1.48 MB** (including rustls TLS engine, tar, gzip, and SHA-256 verification), well below the 6 MB target.

## Micro-Benchmarks & Integration Tests
- 13 automated tests across workspace:
  - `test_image_lifecycle_deduplication_and_gc`: Verifies multi-layer stacking, OCI whiteouts, layer deduplication across images, and GC pruning of orphaned blobs.
  - `test_whiteout_file_removal` and `test_opaque_whiteout_removal`: Unit tests for explicit and opaque whiteouts.
  - All tests passing with 0 warnings on clippy (`-D warnings`) and rustfmt.
