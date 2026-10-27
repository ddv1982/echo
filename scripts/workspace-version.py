#!/usr/bin/env python3
"""Print workspace.package.version from the root Cargo.toml."""
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parent.parent

with (ROOT / "Cargo.toml").open("rb") as manifest:
    print(tomllib.load(manifest)["workspace"]["package"]["version"])
