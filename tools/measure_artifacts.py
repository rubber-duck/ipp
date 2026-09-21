#!/usr/bin/env python3
"""Report reproducible raw/gzip sizes and build identities for explicit artifacts."""

import argparse
import gzip
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+", type=Path)
    args = parser.parse_args()
    records = []
    for path in args.artifacts:
        data = path.read_bytes()
        records.append(
            {
                "path": str(path),
                "bytes": len(data),
                "gzip_bytes": len(gzip.compress(data, compresslevel=9, mtime=0)),
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        )
    print(json.dumps(records, indent=2))


if __name__ == "__main__":
    main()
