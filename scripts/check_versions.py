#!/usr/bin/env python3
"""Verify all rustEZ package manifests advertise the same version.

The workspace ships three coupled artifacts that must agree:

  - ``rustez``    — core Rust crate (``rustez/Cargo.toml``)
  - ``rustez-py`` — PyO3 native crate (``rustez-py/Cargo.toml``)
  - ``rustez``    — Python package metadata (``rustez-py/pyproject.toml``)

Drift between them confuses users, support, and security scanners
because the Python wheel can embed a newer core crate while advertising
an older Python package version. CI runs this script to catch drift
before publish.

Exit code 0 when all versions match, 1 otherwise. The diagnostic prints
each manifest's version so the failure is self-explaining.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

MANIFESTS: list[tuple[str, Path, tuple[str, ...]]] = [
    ("rustez (crate)", REPO_ROOT / "rustez" / "Cargo.toml", ("package", "version")),
    ("rustez-py (crate)", REPO_ROOT / "rustez-py" / "Cargo.toml", ("package", "version")),
    ("rustez (python)", REPO_ROOT / "rustez-py" / "pyproject.toml", ("project", "version")),
]


def read_version(path: Path, keys: tuple[str, ...]) -> str:
    """Return the version string at the given dotted key path inside a TOML file."""
    data = tomllib.loads(path.read_text())
    for key in keys:
        data = data[key]
    return data


def main() -> int:
    """Print each manifest's version and exit non-zero on drift."""
    versions: dict[str, str] = {
        label: read_version(path, keys) for label, path, keys in MANIFESTS
    }

    width = max(len(label) for label in versions)
    for label, version in versions.items():
        print(f"  {label:<{width}}  {version}")

    unique = set(versions.values())
    if len(unique) != 1:
        print(
            f"\nERROR: package versions drift across manifests: {sorted(unique)}",
            file=sys.stderr,
        )
        return 1

    print(f"\nAll manifests agree on version {unique.pop()}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
