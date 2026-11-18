#!/usr/bin/env python3
"""Packaging script for alphen-box on Alphen OS."""

import os
import shutil
import sys
import tarfile

DIST_DIR = "dist"
PKG_ROOT = os.path.join(DIST_DIR, "pkg")

CANDIDATES = [
    "target/aarch64-unknown-linux-musl/release/alphen-box",
    "target/release/alphen-box",
    "target/release/alphen-box.exe",
]

def main():
    binary_source = None
    for c in CANDIDATES:
        if os.path.exists(c):
            binary_source = c
            break

    if not binary_source:
        print("[FAIL] No release binary found to package. Run `cargo build --release` first.", file=sys.stderr)
        sys.exit(1)

    # Clean and create directory structure
    if os.path.exists(DIST_DIR):
        shutil.rmtree(DIST_DIR)

    bin_dir = os.path.join(PKG_ROOT, "usr", "bin")
    systemd_dir = os.path.join(PKG_ROOT, "usr", "lib", "systemd", "system")
    var_blobs = os.path.join(PKG_ROOT, "var", "lib", "alphen-box", "blobs")
    var_images = os.path.join(PKG_ROOT, "var", "lib", "alphen-box", "images")
    var_containers = os.path.join(PKG_ROOT, "var", "lib", "alphen-box", "containers")
    run_dir = os.path.join(PKG_ROOT, "run", "alphen-box")

    for d in [bin_dir, systemd_dir, var_blobs, var_images, var_containers, run_dir]:
        os.makedirs(d, exist_ok=True)

    dest_bin_name = "alphen-box.exe" if binary_source.endswith(".exe") else "alphen-box"
    shutil.copy2(binary_source, os.path.join(bin_dir, dest_bin_name))

    service_file = os.path.join("packaging", "alphen-box.service")
    if os.path.exists(service_file):
        shutil.copy2(service_file, os.path.join(systemd_dir, "alphen-box.service"))

    tar_path = os.path.join(DIST_DIR, "alphen-box-0.1.0-arm64.tar.gz")
    with tarfile.open(tar_path, "w:gz") as tar:
        tar.add(PKG_ROOT, arcname=".")

    print(f"[SUCCESS] Packaged {tar_path} (staged in {PKG_ROOT})")

if __name__ == "__main__":
    main()
