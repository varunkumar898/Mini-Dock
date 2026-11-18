# Milestone 3: Run Without Isolation

## Overview
Milestone 3 implements the container execution pipeline, overlayfs rootfs assembly, and root filesystem transition using Linux `pivot_root(2)` (with `chroot(2)` fallback), allowing `alphen-box` to run container commands such as `/bin/sh` or the container image's default entrypoint.

## What Was Built

1. **OverlayFS Directory Manager (`alphen-runtime::overlay`)**:
   - Manages layered container directories:
     - `lowerdir`: read-only image layers in descending stack order.
     - `upperdir`: container read-write layer where file modifications, creations, and deletions are saved.
     - `workdir`: required overlayfs scratch directory.
     - `merged`: mount point presenting the unified view of the container filesystem.
   - On Linux, calls `libc::mount` with filesystem type `"overlay"` and option `lowerdir=...,upperdir=...,workdir=...`.
   - On exit, safely lazy-unmounts with `umount2(MNT_DETACH)`.

2. **Root Transition Mechanism (`alphen-runtime::pivot`)**:
   - Implements Linux `pivot_root(2)` without relying on insecure `chroot`:
     1. Self-bind-mounts `new_root` onto itself (`MS_BIND | MS_REC`) ensuring it is an independent mount point.
     2. Remounts root as private (`MS_REC | MS_PRIVATE`) preventing host mount pollution.
     3. Creates temporary directory `new_root/.old_root`.
     4. Executes `SYS_pivot_root(new_root, .old_root)` via `libc::syscall`.
     5. Changes directory to `/`.
     6. Detaches old root via `libc::umount2("/.old_root", MNT_DETACH)` and removes the directory.
   - Provides `switch_root_chroot` as a secondary fallback.

3. **Container Supervisor (`alphen-runtime::container`)**:
   - Generates unique 12-character hex container IDs.
   - Resolves command precedence: CLI trailing arguments > Image `ENTRYPOINT` + `CMD` > `/bin/sh`.
   - Populates and propagates standard environment variables (`PATH`, `TERM`, image config `Env`, and CLI `-e KEY=VALUE`).
   - Supports auto-cleanup with `--rm`.

4. **CLI Integration**:
   - `alphen-box run [flags] <image> [cmd...]` is now active.
   - Automatically pulls the image from OCI/Docker registries if not cached locally.

## Binary Size & Budgets
- Release binary measured size: **~1.5 MB** stripped, well within the 6 MB budget.
- 17 automated tests passing cleanly across the workspace.

## Next Steps
- Milestone 4 will add full kernel isolation: PID, mount, UTS, IPC, network, and user namespaces, plus `/proc`, `/sys` (read-only), and minimal `/dev` virtual filesystem mounting.
