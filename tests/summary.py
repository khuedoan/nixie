#!/usr/bin/env python3

import subprocess
import sys
from pathlib import Path


FLAKE = Path("examples")


def hosts_diff(flake):
    result = subprocess.run(
        ["git", "-C", str(flake), "diff", "--no-ext-diff", "--", "hosts.json"],
        capture_output=True,
        text=True,
    )
    return result.stdout.splitlines()


def build_summary(result):
    lines = ["## Nixie E2E Test", "", f"**{result.upper()}**", "", "```diff"]
    lines += hosts_diff(FLAKE)
    lines += ["```", ""]
    return "\n".join(lines) + "\n"


def main():
    result = sys.argv[1] if len(sys.argv) > 1 else "unknown"
    print(build_summary(result), end="")


if __name__ == "__main__":
    main()
