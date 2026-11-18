# Milestone 4: Namespaces and Mounts

## Overview
Milestone 4 introduces full container process and filesystem isolation via Linux kernel namespaces (PID, Mount, UTS, IPC) and virtual filesystems (`/proc`, `/sys`, `/dev`, `/dev/pts`, `/dev/shm`), ensuring processes inside cannot see or manipulate host processes or host filesystems.

## Key Isolation Features Implemented

1. **Linux Namespaces (`alphen-runtime::namespaces`)**:
   - **PID Namespace (`CLONE_NEWPID`)**: The container init process is PID 1 inside the container, completely isolating host process tables.
   - **Mount Namespace (`CLONE_NEWNS`)**: Private mount table with `MS_REC | MS_PRIVATE` preventing mount events from propagating to the host.
   - **UTS Namespace (`CLONE_NEWUTS`)**: Isolates hostname and NIS domain via `sethostname(2)`. Hostname defaults to container ID or `--hostname`.
   - **IPC Namespace (`CLONE_NEWIPC`)**: Isolates System V IPC keys and POSIX message queues.

2. **Virtual Filesystem Setup (`alphen-runtime::mounts`)**:
   - `/proc`: Mounted privately (`procfs`). Inside the container, `ps` displays only PID 1 and container processes.
   - `/sys`: Mounted read-only (`sysfs`) with `MS_RDONLY | MS_NODEV | MS_NOSUID | MS_NOEXEC`.
   - `/dev`: Minimal device node tmpfs (`mode=755`) exposing only `null`, `zero`, `full`, `random`, `urandom`, and `tty`.
   - `/dev/pts`: Mounted with `newinstance,ptmxmode=0666,mode=0620` for isolated pseudoterminals.
   - `/dev/shm`: Isolated shared memory tmpfs (`mode=1777`).

3. **User Mounts & Security Flags**:
   - **Bind Mounts (`-v /host:/container[:ro]`)**: Recursively bind-mounts host directories/files into the container, optionally marking them read-only with `MS_REMOUNT | MS_RDONLY`.
   - **Tmpfs Mounts (`--tmpfs /path`)**: Mounts ephemeral tmpfs filesystems.
   - **Read-Only Root (`--read-only`)**: Remounts root filesystem as read-only.

## Binary Size & Budgets
```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    1,720,320 bytes (1680.0 KB / 1.64 MB)
 Budget (M4):      6,291,456 bytes (6.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (1.64 MB <= 6.0 MB)
```
- Release binary size: **1.64 MB**, well under the 6 MB target.
- 19 automated tests passing across the workspace.
