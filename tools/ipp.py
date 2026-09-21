#!/usr/bin/env python3
"""IPP development commands. Help and planning require only Python's standard library."""

from pathlib import Path
import sys

required = (Path(__file__).parent.parent / ".python-version").read_text().strip()
if f"{sys.version_info.major}.{sys.version_info.minor}" != required:
    raise SystemExit(f"IPP requires Python {required}; found {sys.version.split()[0]}")

from pipeline.cli import main


if __name__ == "__main__":
    raise SystemExit(main())
