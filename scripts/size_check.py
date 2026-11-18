#!/usr/bin/env python3
"""Binary size check utility for alphen-box."""

import os
import sys

BUDGET_BYTES = 2 * 1024 * 1024  # Milestone 1: 2 MB hard target

CANDIDATES = [
    "target/aarch64-unknown-linux-musl/release/alphen-box",
    "target/release/alphen-box",
    "target/release/alphen-box.exe",
]

def main():
    found = None
    for c in CANDIDATES:
        if os.path.exists(c):
            found = c
            break

    if not found:
        print("[FAIL] No release binary found. Run `cargo build --release` first.", file=sys.stderr)
        sys.exit(1)

    size = os.path.getsize(found)
    size_mb = size / (1024 * 1024)
    size_kb = size / 1024
    budget_mb = BUDGET_BYTES / (1024 * 1024)

    print("==================================================")
    print(" alphen-box Binary Size Verification")
    print("==================================================")
    print(f" Binary Path:      {found}")
    print(f" Measured Size:    {size:,} bytes ({size_kb:.1f} KB / {size_mb:.2f} MB)")
    print(f" Budget (M1):      {BUDGET_BYTES:,} bytes ({budget_mb:.1f} MB)")
    print("--------------------------------------------------")

    if size <= BUDGET_BYTES:
        print(f" [PASS] Binary is within budget ({size_mb:.2f} MB <= {budget_mb:.1f} MB)")
        sys.exit(0)
    else:
        print(f" [FAIL] Binary exceeds budget ({size_mb:.2f} MB > {budget_mb:.1f} MB)", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
