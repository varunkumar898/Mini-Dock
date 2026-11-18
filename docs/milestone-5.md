# Milestone 5: cgroups v2 Resource Limits

## Overview
Milestone 5 implements Linux cgroups v2 resource controls for container workloads, enforcing memory limits (hard limits and swap limits), fractional CPU quotas and weights, process ID count limits, and block I/O throttling.

Kernel documentation reference: `Documentation/admin-guide/cgroup-v2.rst`.

## Resource Controls Implemented

1. **cgroups v2 Hierarchy Management (`alphen-runtime::cgroups`)**:
   - Container cgroups are organized under `/sys/fs/cgroup/alphen-box/<container-id>/`.
   - Subtree controllers (`+cpu +memory +pids +io`) are automatically activated on root and parent cgroups.
   - Container PIDs are attached via `cgroup.procs`.
   - Cgroup directories are automatically cleaned up (`rmdir`) on container termination.

2. **Memory Limits (`memory.max`, `memory.swap.max`)**:
   - `--memory <limit>`: Accepts human-friendly notation (`64m`, `512k`, `1g`) and writes bytes to `memory.max`. When memory usage exceeds this limit and cannot be reclaimed, the kernel OOM killer triggers.
   - `--memory-swap <limit>`: Sets swap consumption threshold in `memory.swap.max`.

3. **CPU Quotas & Shares (`cpu.max`, `cpu.weight`)**:
   - `--cpus <fraction>`: Calculates quota against a default period of 100,000 µs (100 ms) and writes `$quota $period` to `cpu.max` (e.g. `--cpus 1.5` -> `150000 100000`).
   - `--cpu-shares <weight>`: Maps standard Docker shares (2-262144, default 1024) onto cgroups v2 proportional `cpu.weight` (1-10000, default 100).

4. **PID Limits (`pids.max`)**:
   - `--pids-limit <count>`: Prevents fork bombs and runaway process creation by writing to `pids.max`.

5. **Block I/O Throttling (`io.max`)**:
   - `--device-read-bps` and `--device-write-bps`: Restricts maximum read/write throughput per block device.

## Binary Size & Budgets
```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    1,703,936 bytes (1664.0 KB / 1.63 MB)
 Budget (M5):      6,291,456 bytes (6.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (1.63 MB <= 6.0 MB)
```
- Release binary size: **1.63 MB**, well within the 6 MB hard target.
- 21 automated tests passing across the workspace.
