#!/bin/sh
# alphen-box-setup.sh - Boot-time and pre-flight initialization for Alphen OS
set -eu

echo "[alphen-box] Running boot-time system setup..."

# 1. Verify kernel modules
for mod in overlay br_netfilter tun; do
    if ! lsmod | grep -q "^$mod "; then
        echo "[alphen-box] Loading kernel module: $mod"
        modprobe "$mod" 2>/dev/null || true
    fi
done

# 2. Configure kernel sysctl parameters
sysctl -w net.ipv4.ip_forward=1 >/dev/null 2>&1 || true
sysctl -w net.ipv4.conf.all.route_localnet=1 >/dev/null 2>&1 || true
sysctl -w kernel.unprivileged_userns_clone=1 >/dev/null 2>&1 || true

# 3. Ensure required directories exist with correct permissions
mkdir -p /var/lib/alphen-box/blobs/sha256
mkdir -p /var/lib/alphen-box/images
mkdir -p /var/lib/alphen-box/containers
mkdir -p /var/lib/alphen-box/networks
mkdir -p /run/alphen-box

chmod 755 /var/lib/alphen-box
chmod 755 /run/alphen-box

echo "[alphen-box] Setup complete. Environment ready."
