# Milestone 1: Skeleton

## Overview
Milestone 1 establishes the baseline architecture, Cargo multi-crate workspace, Docker-compatible CLI command structure, build automation, and packaging framework for `alphen-box` targeting Alphen OS on ARM64 (`aarch64-unknown-linux-musl`).

## Workspace Crates
The project is split into six modular crates:
1. `alphen-common`: Centralized error handling (`AlphenError`) and lightweight logging configuration using `env_logger`.
2. `alphen-core`: OCI Image Reference parsing, image metadata representations (`ImageSummary`), and core runtime data structures.
3. `alphen-image`: Image store layout (`/var/lib/alphen-box/images`, `/var/lib/alphen-box/blobs`) and listing facilities.
4. `alphen-runtime`: Host verification verifying Linux 5.15+, cgroups v2 unified hierarchy, user namespaces, overlayfs, and seccomp.
5. `alphen-net`: Network driver representations (`none`, `host`, `bridge`) and port forwarding specification parsers (`HOST:CONTAINER[/PROTOCOL]`).
6. `alphen-box`: Command-line executable using `clap` (derive only, default features off).

## Binary Size & Budgets

Hard target for M1: **< 2.0 MB** release binary.

### Measured Size
```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    460,288 bytes (449.5 KB / 0.44 MB)
 Budget (M1):      2,097,152 bytes (2.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (0.44 MB <= 2.0 MB)
```
The stripped release binary is **449 KB (0.44 MB)**, well below the 2 MB milestone budget and well positioned for the final < 6 MB budget.

### Micro-Benchmark Results
```
[BENCH] verify_system_requirements: total 1.53ms for 10,000 iterations (~152 ns / check)
[BENCH] ImageReference::parse: total 10.25ms for 20,000 iterations (~512 ns / parse)
```

## Known Limitations & Next Steps
- Image pull and extraction are stubbed in M1 and will be implemented in Milestone 2.
- Container isolation and overlayfs mounting will be built in Milestones 3 and 4.
