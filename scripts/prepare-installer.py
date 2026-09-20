#!/usr/bin/env python3
"""Pin the same build-time public key into the published bootstrap installer."""
import base64
import os
from pathlib import Path
import sys


def render(public_hex: str) -> str:
    key = bytes.fromhex(public_hex.strip())
    if len(key) != 32 or key.hex() == "247f88e163242986b7107eb704a983e12186d2697a3927b7e6b42ec2b364272b":
        raise ValueError("a new 32-byte production public key is required")
    der = bytes.fromhex("302a300506032b6570032100") + key
    pem = "-----BEGIN PUBLIC KEY-----\n" + base64.b64encode(der).decode() + "\n-----END PUBLIC KEY-----"
    template = Path(__file__).with_name("install.sh").read_text(encoding="utf-8")
    return template.replace("@SBCTL_RELEASE_PUBLIC_KEY_PEM@", pem)


if __name__ == "__main__":
    output = Path(sys.argv[1])
    output.write_text(render(os.environ["SBCTL_RELEASE_PUBLIC_KEY_HEX"]), encoding="utf-8", newline="\n")
