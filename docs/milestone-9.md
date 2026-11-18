# Milestone 9: Rootless Mode

## Overview
Milestone 9 implements first-class rootless execution for `alphen-box`, allowing unprivileged non-root users to pull, unpack, isolate, and execute OCI containers without `sudo`, background daemon processes, or setuid helper binaries (`newuidmap`/`newgidmap`).

## Features Implemented

1. **User Namespace Architecture (`alphen-runtime::rootless`)**:
   - `CLONE_NEWUSER` unshare: allocates a private user namespace where the calling user gains virtual root (`UID 0`) inside the container.
   - `deny` written to `/proc/<pid>/setgroups`: satisfies Linux kernel security requirements for unprivileged `gid_map` manipulation without requiring root or setuid helpers.
   - Direct single-range ID mapping:
     - `/proc/<pid>/uid_map`: `0 <host_uid> 1`
     - `/proc/<pid>/gid_map`: `0 <host_gid> 1`

2. **Automatic Rootless Detection**:
   - Runtime invokes `is_rootless()` (`geteuid() != 0`).
   - If non-root, rootless user namespace is enabled automatically without requiring explicit flags.
   - CLI provides explicit `--rootless` flag to force rootless semantics.

3. **Unprivileged OverlayFS**:
   - Uses `userxattr` mount option available in Linux 5.11+ kernels, enabling unprivileged user namespaces to write overlay extended attributes under the `user.` namespace instead of `trusted.`.

## Root vs Rootless Feature Matrix

| Feature | Root Mode | Rootless Mode | Notes |
|---|---|---|---|
| **OCI Image Pull & Storage** | `/var/lib/alphen-box/` | `~/.local/share/alphen-box/` | Unprivileged blob storage |
| **User Namespaces** | Optional | **Always Enabled** | Maps host UID $\to$ UID 0 |
| **Mount Namespace & Pivot Root** | Standard `pivot_root` | User Namespace `pivot_root` | Unprivileged root transition |
| **Host Bridge Network (`alphen0`)** | Full netlink configuration | Requires `CAP_NET_ADMIN` on host | In rootless mode, use `--network host` or pre-configured tap device |
| **Container Networking (`none`)** | Full loopback | Full loopback | Fully supported |
| **cgroups v2 Resource Limits** | Fully enforced | Supported via systemd user slice (`/sys/fs/cgroup/user.slice/...`) | Requires user slice delegation |
| **Port Forwarding (< 1024)** | Allowed | Requires sysctl `ip_unprivileged_port_start` $\le$ port | Non-privileged ports ($\ge 1024$) work natively |

## Binary Size & Budgets
```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    1,707,008 bytes (1667.0 KB / 1.63 MB)
 Budget (M9):      6,291,456 bytes (6.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (1.63 MB <= 6.0 MB)
```
- Measured release binary size: **1.63 MB**, well within the 6.0 MB budget.
- 41 automated tests passing across the workspace.
