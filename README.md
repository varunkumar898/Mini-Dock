# Mini-Dock (`alphen-box`)

> **Ultra-lightweight, 1.68 MB daemonless container runtime designed for ARM64 Linux and 1 GB RAM SBCs (Raspberry Pi 4/5, ARM servers, Alphen OS) and Windows on ARM.**

A high-performance, drop-in Docker replacement built from scratch in Rust. Runs existing, unmodified OCI / Docker images with minimal memory, zero background daemon overhead, and sub-50 ms startup latency.

---

## ⚡ Why Mini-Dock instead of Docker?

| Metric | Docker CE (v26.1) | Mini-Dock (`alphen-box`) | Advantage |
|---|---|---|---|
| **Binary Footprint** | 169.5 MB (`dockerd` + `containerd` + `runc` + `docker`) | **1.68 MB** (single static binary) | **98.8% smaller** |
| **Idle Memory (RSS)** | **84.6 MB** (always resident background daemons) | **0.0 MB** (daemonless architecture) | **Saves ~85 MB RAM on 1GB SBCs** |
| **Container Startup (Pi 4)** | 425 ms | **48.2 ms** | **8.8× faster** |
| **Container Startup (Pi 5)** | 238 ms | **26.4 ms** | **9.0× faster** |
| **Networking Overhead** | Forks external `iptables` / `ip` / `bridge` binaries | **Direct raw Netlink & nftables** sockets | Zero process spawning overhead |
| **Security Defaults** | Broad capabilities, default permissive | **Capabilities dropped by default**, Seccomp BPF, `no_new_privs` | Hardened against breakouts |
| **Rootless Execution** | Requires rootlesskit + slirp4netns + extra daemons | **Native `CLONE_NEWUSER` + `userxattr` OverlayFS** | Zero-setup rootless mode |

Detailed hardware benchmarks on Raspberry Pi 4 (Cortex-A72) and Raspberry Pi 5 (Cortex-A76) are documented in [BENCHMARKS.md](BENCHMARKS.md).

---

## 🚀 Quickstart

### 1. Build from Source
Mini-Dock requires Rust 1.80+ (`cargo`):

```bash
git clone https://github.com/varunkumar898/Mini-Dock.git
cd Mini-Dock

# Build release binaries (alphen-box and docker compatibility wrapper)
cargo build --release
```

Binaries will be placed in `target/release/alphen-box` and `target/release/docker`.

### 2. Pre-flight Setup (Linux ARM64 / Alphen OS)
Run the automated pre-flight setup to configure kernel prerequisites (`overlay`, `br_netfilter`, `tun`, and sysctls):

```bash
sudo ./packaging/alphen-box-setup.sh
```

### 3. Install System-Wide
```bash
sudo cp target/release/alphen-box /usr/local/bin/alphen-box
sudo cp target/release/docker /usr/local/bin/docker
```

Now you can use `alphen-box` or `docker` interchangeably in all your scripts!

---

## 📦 Using Mini-Dock in Your Next Project

Mini-Dock provides complete Docker CLI compatibility:

### Pulling an Image
```bash
alphen-box pull alpine:3.20
# Or using the docker wrapper:
docker pull nginx:alpine
```

### Running Containers
```bash
# Run interactive shell
docker run -it --rm alpine:3.20 sh

# Run detached web server with port publishing
docker run -d --name my-web -p 8080:80 nginx:alpine

# Run with resource limits (cgroups v2)
docker run --rm --memory 64m --cpus 1.5 alpine:3.20 sh -c "echo 'Running constrained'"

# Run with volume mounts and tmpfs
docker run -v /home/user/app:/app:ro --tmpfs /tmp alpine:3.20 ls -la /app

# Run in rootless mode as an unprivileged user
alphen-box run --rootless --rm alpine:3.20 whoami
```

### Managing Containers
```bash
# List active and stopped containers
docker ps -a

# View container logs (with live follow -f)
docker logs -f my-web

# Execute a command in a running container
docker exec -it my-web /bin/sh

# Stop and remove containers
docker stop my-web
docker rm my-web

# List local images and garbage collect unused blobs
docker images
alphen-box gc
```

---

## 🛠️ Architecture

Mini-Dock is built as a modular Cargo workspace:

```
Mini-Dock/
├── crates/
│   ├── common/       # Typed AlphenError, logger, and shared utility macros
│   ├── core/         # OCI Image/Distribution spec models, ContainerState
│   ├── image/        # OCI registry client (TLS via rustls), layer deduplication, whiteouts
│   ├── runtime/      # OverlayFS driver, pivot_root, namespaces, cgroups v2, seccomp BPF
│   ├── net/          # Direct kernel Netlink route driver, IPAM (10.88.0.0/16), nftables NAT, DNS
│   └── cli/          # clap-derived CLI binaries (alphen-box and docker wrapper)
├── packaging/        # Systemd unit, boot setup script, APKBUILD, and Debian packages
├── docs/             # Milestone documentation from M1 to M11
├── BENCHMARKS.md     # Comparative benchmark analysis on Raspberry Pi 4/5
└── CHANGELOG.md      # Full release history from v0.1.0 to v1.0.0
```

### Key Technical Innovations
- **Synchronous Direct Netlink Route & Netfilter**: Zero dependency on external network CLI tools (`ip`, `ifconfig`, `iptables`, `brctl`). All interface creation (`alphen0`), veth management, IP assignment, and nftables DNAT rules are sent directly to kernel sockets via `libc::socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE | NETLINK_NETFILTER)`.
- **Daemonless Supervisor**: No long-running background daemon process eating memory when containers aren't running. When a container runs detached (`-d`), lightweight process monitors supervise state and write stdout/stderr to rotating logs (10 MB × 3 files).
- **Hardened by Default**: Drops all Linux capabilities from the bounding set by default (`--cap-add` to restore), blocks dangerous kernel syscalls via Seccomp BPF, and enforces `PR_SET_NO_NEW_PRIVS`.
- **First-Class Rootless Operation**: Uses single-mapping `CLONE_NEWUSER` with `setgroups` denial to run without `sudo` or setuid helpers (`newuidmap`/`newgidmap`), accompanied by `userxattr` OverlayFS.

---

## 📋 Full Command Reference

| Command | Description |
|---|---|
| `alphen-box pull <image>` | Pulls OCI/Docker image by tag or SHA-256 digest with multi-arch resolution. |
| `alphen-box run [flags] <image> [cmd]` | Runs container with isolation, cgroups v2, networking, and volume mounts. |
| `alphen-box ps [-a]` | Lists running (or all) containers with ID, image, status, and ports. |
| `alphen-box logs [-f] <container>` | Inspects rotating logs (stdout/stderr) with optional follow. |
| `alphen-box exec [-it] <container> <cmd>` | Joins container namespaces via `setns(2)` to run debugging commands. |
| `alphen-box stop <container>` | Graceful SIGTERM with timeout escalating to SIGKILL. |
| `alphen-box start <container>` | Relaunches previously stopped container. |
| `alphen-box rm [-f] <container>` | Removes container rootfs, logs, and metadata. |
| `alphen-box inspect <container>` | Returns detailed runtime configuration and state as JSON. |
| `alphen-box images` | Lists locally cached images and total layer sizes. |
| `alphen-box rmi <image>` | Removes stored image reference. |
| `alphen-box gc` | Prunes unreferenced content-addressed layer blobs from store. |
| `alphen-box version` | Prints runtime version and system architecture info. |

---

## 📜 License

Licensed under the Apache License, Version 2.0.
