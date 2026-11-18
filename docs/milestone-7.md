# Milestone 7: Networking

## Overview
Milestone 7 delivers container networking without relying on external utilities or daemons (`ip`, `ifconfig`, `iptables`, `dockerd`). It speaks directly to the Linux kernel via synchronous `AF_NETLINK` sockets (`NETLINK_ROUTE` and `NETLINK_NETFILTER`), implements an IP Address Management (IPAM) allocator for `10.88.0.0/16`, creates and configures bridge `alphen0` and virtual ethernet (`veth`) pairs, provisions `nftables` masquerade and port forwarding (DNAT), and injects container `/etc/resolv.conf`.

## Features Implemented

1. **Direct Netlink Link & Routing Driver (`alphen-net::netlink`)**:
   - Zero-dependency synchronous raw Netlink socket (`libc::AF_NETLINK`, `libc::NETLINK_ROUTE`).
   - Automated `alphen0` bridge device creation (`RTM_NEWLINK` with nested `IFLA_LINKINFO` kind="bridge").
   - Veth pair generation (`veth_<short_id>` on host $\leftrightarrow$ `ceth_<short_id>` in container).
   - Link master assignment (`IFLA_MASTER` pointing to `alphen0`).
   - Atomic interface namespace migration (`IFLA_NET_NS_PID`) into the container's isolated network namespace (`CLONE_NEWNET`).
   - Direct address assignment (`RTM_NEWADDR` for `10.88.0.1/16` gateway and container IPs) and default routing (`RTM_NEWROUTE` `0.0.0.0/0 via 10.88.0.1`).
   - Clean veth teardown on container exit (`RTM_DELLINK`).

2. **Deterministic IPAM Allocator (`alphen-net::ipam`)**:
   - Manages subnet `10.88.0.0/16` (`10.88.0.1` bridge gateway, `10.88.0.2`–`10.88.255.254` allocatable).
   - Atomic lease records under `/var/lib/alphen-box/networks/ipam.json` with `flock` file locking.
   - Lowest-available IP address reuse and deterministic query by container ID.

3. **nftables NAT & Port Forwarding (`alphen-net::nftables`)**:
   - Talks directly to kernel netfilter (`NETLINK_NETFILTER`) without `iptables` or shelling out.
   - Sets up table `alphen_box` with outbound IPv4 masquerade on the bridge.
   - DNAT port forwarding for `-p HOST:CONTAINER[/tcp|udp]`.

4. **DNS Generation (`alphen-net::dns`)**:
   - Discovers host DNS nameservers while filtering out unroutable loopback stubs (`127.0.0.53`).
   - Injects `/etc/resolv.conf` into container rootfs with support for explicit `--dns <ip>` overrides.

5. **Network Modes Supported**:
   - `--network host`: Shares host network namespace directly (no `CLONE_NEWNET`).
   - `--network none`: Fully isolated network namespace with loopback only.
   - `--network bridge` (default): Connected to `alphen0` via veth pair, with IPAM lease and NAT.

## Binary Size & Budgets
```
==================================================
 alphen-box Binary Size Verification
==================================================
 Binary Path:      target/release/alphen-box.exe
 Measured Size:    1,707,008 bytes (1667.0 KB / 1.63 MB)
 Budget (M7):      6,291,456 bytes (6.0 MB)
--------------------------------------------------
 [PASS] Binary is within budget (1.63 MB <= 6.0 MB)
```
- Measured release binary size: **1.63 MB**, well within the 6.0 MB budget.
- 31 automated tests passing across the workspace.
