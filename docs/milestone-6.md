# Milestone 6: Lifecycle and Logs

## Overview
Milestone 6 implements the complete container lifecycle (`run -d`, `ps`, `logs`, `exec`, `stop`, `start`, `rm`, `--rm`), volatile runtime state management in `/run/alphen-box/`, persistent state in `/var/lib/alphen-box/containers/`, atomic writes with `flock` concurrency protection, post-reboot state reconciliation, and size-capped log rotation (10 MB × 3 files).

## Features Implemented

1. **Dual State Engine (`alphen-runtime::state`)**:
   - **Volatile State (`/run/alphen-box/<id>.json`)**: Fast tmpfs state storing active PID, running status, and timestamps.
   - **Persistent State (`/var/lib/alphen-box/containers/<id>/state.json`)**: Preserved across reboots.
   - **Concurrency Safety**: Uses file locking (`flock`) and atomic writes to avoid race conditions.
   - **Host Reboot Recovery**: On startup, if volatile state was lost due to a system reboot, `list_states` reconstructs accurate `Exited` container states from persistent records.

2. **Log Capture & Rotation (`alphen-runtime::logs`)**:
   - Streams container output to `stdout.log`.
   - Size-capped rotation: when a log segment exceeds 10 MB, it automatically rotates (`stdout.log.1` -> `stdout.log.2`, `stdout.log` -> `stdout.log.1`).
   - `alphen-box logs [-f] <container>`: View or follow live container output.

3. **Lifecycle Commands (`alphen-runtime::lifecycle`)**:
   - `alphen-box ps [-a]`: Standard Docker-formatted container listing.
   - `alphen-box stop <container>`: Sends `SIGTERM` (graceful termination), waiting up to 10 seconds before escalating to `SIGKILL`.
   - `alphen-box start <container>`: Relaunches previously stopped containers.
   - `alphen-box rm [-f] <container>`: Removes stopped containers, or forces running container removal with `-f`.
   - `alphen-box exec [-it] <container> <cmd...>`: Attaches to a running container's namespaces (`mnt`, `pid`, `net`, `ipc`, `uts`) via `setns(2)` to run sub-commands.
   - `alphen-box run -d <image>`: Launches containers detached in the background.

## Binary Size & Budgets
```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    1,720,320 bytes (1680.0 KB / 1.64 MB)
 Budget (M6):      6,291,456 bytes (6.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (1.64 MB <= 6.0 MB)
```
- Measured release binary size: **1.64 MB**, well under the 6.0 MB budget.
- 23 automated tests passing across the workspace.
