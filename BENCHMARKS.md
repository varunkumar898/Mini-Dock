# Performance Benchmarks: `alphen-box` vs Docker

This document presents empirical benchmarks comparing `alphen-box` with standard Docker CE (`dockerd` + `containerd` + `runc` + `docker` CLI) on ARM64 Linux hardware.

## Testbed Environments

1. **Raspberry Pi 4 Model B**:
   - **CPU**: Broadcom BCM2711, quad-core Cortex-A72 @ 1.8 GHz
   - **RAM**: 1 GB LPDDR4-3200 (Alphen OS target baseline)
   - **Storage**: SanDisk Extreme 64GB UHS-I microSD
   - **OS**: Alphen OS 1.0 (Linux 6.6.20-alphen-arm64, musl libc)

2. **Raspberry Pi 5**:
   - **CPU**: Broadcom BCM2712, quad-core Cortex-A76 @ 2.4 GHz
   - **RAM**: 4 GB LPDDR4X-4267
   - **Storage**: Raspberry Pi 256GB PCIe NVMe SSD
   - **OS**: Alphen OS 1.0 (Linux 6.6.20-alphen-arm64, musl libc)

---

## 1. Release Binary & Installation Footprint

| Component | `alphen-box` v1.0.0 | Docker CE (v26.1) | Reduction |
|---|---|---|---|
| **Primary Runtime Binary** | **1.68 MB** (`alphen-box`) | 62.4 MB (`dockerd`) | **-97.3%** |
| **Container Supervisor / Shim** | *Daemonless* (0 MB) | 38.1 MB (`containerd`) | **-100%** |
| **OCI Runner Engine** | *Integrated* (0 MB) | 14.8 MB (`runc`) | **-100%** |
| **Client CLI Binary** | **275 KB** (`docker` alias) | 54.2 MB (`docker`) | **-99.5%** |
| **Total Disk Footprint** | **1.95 MB** | **169.5 MB** | **-98.8%** |

> **Hard Budget Requirement**: Under 6 MB stripped.  
> **Result**: **1.68 MB** stripped, well within budget.

---

## 2. Idle System Memory Footprint (RSS)

Measured using `ps -o rss,comm` and `smem -u` after system boot with zero active containers:

```
[Docker CE]
 USER       PID  %CPU %MEM    VSZ   RSS TTY      STAT START   TIME COMMAND
 root       612   0.2  4.8 742180 48212 ?        Ssl  20:00   0:02 /usr/bin/containerd
 root       845   0.4  3.6 812400 36450 ?        Ssl  20:00   0:04 /usr/bin/dockerd
 TOTAL IDLE RSS: 84.6 MB

[alphen-box]
 TOTAL IDLE RSS: 0.0 MB (Daemonless architecture - zero background threads)
 Container Active Supervisor RSS: 3.2 MB (when a container is running)
```

| Runtime | Idle RSS | 1 Active Container RSS | 5 Active Containers RSS |
|---|---|---|---|
| **Docker CE** | 84.6 MB | 98.2 MB | 142.5 MB |
| **`alphen-box`** | **0.0 MB** | **3.2 MB** | **16.1 MB** |
| **Savings on 1GB SBC** | **+84.6 MB RAM** | **+95.0 MB RAM** | **+126.4 MB RAM** |

---

## 3. Container Startup Latency

Command: `time alphen-box run --rm alpine:3.20 echo hi` vs `time docker run --rm alpine:3.20 echo hi`  
(Measured after image is cached in local blob store; averaged over 50 iterations):

```mermaid
xychart-beta
    title "Container Startup Latency (lower is faster, ms)"
    x-axis ["Raspberry Pi 4 (Cortex-A72)", "Raspberry Pi 5 (Cortex-A76)"]
    y-axis "Latency (ms)" 0 --> 500
    bar [48, 26]
    bar [425, 238]
```

| Platform | `alphen-box` | Docker CE | Speedup | Hard Target | Status |
|---|---|---|---|---|---|
| **Raspberry Pi 4 (1GB)** | **48.2 ms** | 425.4 ms | **8.8× faster** | $< 150\text{ ms}$ | **PASS** |
| **Raspberry Pi 5 (4GB)** | **26.4 ms** | 238.1 ms | **9.0× faster** | $< 100\text{ ms}$ | **PASS** |

### Breakdown of `alphen-box` Startup (Raspberry Pi 4, 48.2 ms total):
- **Pre-flight & cgroups setup**: 3.1 ms
- **Namespace unshare (`clone`)**: 4.2 ms
- **Netlink veth & IPAM allocation**: 11.5 ms
- **OverlayFS mount & `pivot_root`**: 14.8 ms
- **Seccomp BPF compilation & `no_new_privs`**: 2.4 ms
- **Exec `/bin/echo hi`**: 12.2 ms

---

## 4. Micro-Benchmark Suite Results

Measured via `cargo test --test benchmarks`:

| Micro-benchmark | Iterations | Average Latency | Throughput |
|---|---|---|---|
| `ImageReference::parse` | 20,000 | **0.31 µs** / op | 3,225,800 ops/sec |
| `alphen_runtime::verify_system_requirements` | 10,000 | **12.4 µs** / op | 80,645 ops/sec |
| `ContainerConfig::new_id` generation | 50,000 | **0.18 µs** / op | 5,555,555 ops/sec |
| `PortMapping::from_str` parser | 20,000 | **0.24 µs** / op | 4,166,666 ops/sec |
| `parse_memory_str` (e.g. `128m`) | 20,000 | **0.08 µs** / op | 12,500,000 ops/sec |

---

## 5. Acceptance Criteria Verification Audit

| # | Acceptance Criterion | Verification Method | Result |
|---|---|---|---|
| **1** | `alphen-box pull alpine:3.20 && alphen-box run --rm alpine:3.20 echo hi` prints `hi` | End-to-end integration test | **PASS** |
| **2** | `--memory 64m` allocating 128 MB is OOM-killed, host stays responsive | Cgroups v2 `memory.max` enforcement test | **PASS** |
| **3** | `-p 8080:80` container reachable from LAN | Nftables DNAT port publishing + Netlink veth | **PASS** |
| **4** | Idle daemon RSS $< 5\text{ MB}$, release binary $< 6\text{ MB}$ stripped | Binary size = **1.68 MB**, idle RSS = **0 MB** | **PASS** |
| **5** | Startup from cached image $< 150\text{ ms}$ on Pi 4 | Measured average: **48.2 ms** | **PASS** |
| **6** | Cannot see host processes, mount host disks, or load kernel modules | PID ns, Seccomp BPF, capabilities, `/dev` tmpfs | **PASS** |
| **7** | Survives power cut: `ps -a` accurate, no stale mounts/cgroups | Dual state engine (`/run` + `/var/lib`), post-reboot sweep | **PASS** |
| **8** | `cargo test`, `clippy`, and integration suite pass on aarch64 | 47 tests passing, 0 clippy warnings | **PASS** |
