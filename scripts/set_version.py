"""Bump the version in pixi.toml and Cargo.toml.

Usage:
    python scripts/set_version.py 0.2.0
"""

import re
import subprocess
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <version>", file=sys.stderr)
        sys.exit(1)

    version = sys.argv[1]

    # Validate version looks reasonable (digits and dots).
    if not re.match(r"^\d+\.\d+\.\d+", version):
        print(f"Invalid version: {version!r} (expected e.g. 1.2.3)", file=sys.stderr)
        sys.exit(1)

    # 1. Update pixi.toml via pixi CLI.
    subprocess.run(
        ["pixi", "project", "version", "set", version],
        check=True,
    )
    print(f"pixi.toml → {version}")

    # 2. Update Cargo.toml (first `version = "..."` in [package]).
    cargo_toml = Path("Cargo.toml")
    text = cargo_toml.read_text()
    new_text, count = re.subn(
        r'^(version\s*=\s*)"[^"]+"',
        rf'\g<1>"{version}"',
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if count == 0:
        print("Warning: no version field found in Cargo.toml", file=sys.stderr)
    else:
        cargo_toml.write_text(new_text)
        print(f"Cargo.toml → {version}")

    # 3. Regenerate Cargo.lock.
    subprocess.run(["cargo", "generate-lockfile"], check=True)
    print("Cargo.lock updated")


if __name__ == "__main__":
    main()
