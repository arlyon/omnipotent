#!/usr/bin/env python3
"""Build the C6 firmware using the active personal Wi-Fi profile, without logging keys."""

import argparse
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIRMWARE = ROOT / "firmware/esp32c6"


def nm(*args):
    try:
        result = subprocess.run(
            ["nmcli", "--escape", "no", *args], capture_output=True, text=True, timeout=15, check=False
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SystemExit("Cannot read NetworkManager; no credential values were printed.") from error
    if result.returncode:
        raise SystemExit("NetworkManager did not return the requested profile field.")
    return result.stdout.rstrip("\n")


def wifi_environment():
    profiles = []
    for line in nm("-t", "-f", "UUID,TYPE", "connection", "show", "--active").splitlines():
        uuid, kind = line.split(":", 1)
        if kind in ("802-11-wireless", "wifi"):
            profiles.append(uuid)
    if len(profiles) != 1:
        raise SystemExit("Expected exactly one active Wi-Fi profile; select one before building.")
    uuid = profiles[0]
    ssid = nm("-g", "802-11-wireless.ssid", "connection", "show", uuid)
    mode = nm("-g", "802-11-wireless-security.key-mgmt", "connection", "show", uuid)
    if mode not in ("wpa-psk", "sae"):
        raise SystemExit("The first firmware supports personal WPA Wi-Fi, not enterprise authentication.")
    password = nm("--show-secrets", "-g", "802-11-wireless-security.psk", "connection", "show", uuid)
    if not ssid or len(ssid.encode("utf-8")) > 32 or not password:
        raise SystemExit("Saved Wi-Fi credentials are missing or the SSID exceeds 32 bytes.")
    return ssid, password


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clippy", action="store_true", help="run strict cross-target Clippy instead")
    args = parser.parse_args()
    ssid, password = wifi_environment()
    environment = os.environ.copy()
    environment.update({
        "OMNIPOTENT_WIFI_SSID": ssid,
        "OMNIPOTENT_WIFI_PASSWORD": password,
        # Never send a credential-bearing compilation to distributed compilers.
        "RUSTC_WRAPPER": "",
        "RUSTC_WORKSPACE_WRAPPER": "",
        "SCCACHE_DISABLE": "1",
        "CARGO_TARGET_DIR": str(ROOT / ".wt/opds/target-c6"),
        "CARGO_BUILD_BUILD_DIR": str(ROOT / ".wt/opds/build-c6"),
        "CARGO_INCREMENTAL": "0",
    })
    environment.pop("CARGO_LOG", None)
    command = ["cargo", "clippy" if args.clippy else "build", "--locked", "--release"]
    if args.clippy:
        command += ["--", "-D", "warnings"]
    print("Using saved Wi-Fi credentials locally; values withheld. Build artifacts contain the key.", flush=True)
    old_umask = os.umask(0o077)
    try:
        result = subprocess.run(command, cwd=FIRMWARE, env=environment, check=False)
    except OSError as error:
        raise SystemExit("Could not start Cargo.") from error
    finally:
        os.umask(old_umask)
    if result.returncode:
        raise SystemExit(result.returncode)
    if not args.clippy:
        print(ROOT / ".wt/opds/target-c6/riscv32imac-unknown-none-elf/release/omnipotent-esp32c6")


if __name__ == "__main__":
    main()
