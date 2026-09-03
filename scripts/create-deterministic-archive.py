#!/usr/bin/env python3
"""Write a reproducible gzip tar archive without GNU tar dependencies."""

from __future__ import annotations

import gzip
import os
import sys
import tarfile
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 4:
        print(f"usage: {sys.argv[0]} STAGE PACKAGE OUTPUT", file=sys.stderr)
        return 2

    stage, package, output = map(Path, sys.argv[1:])
    root = stage / package
    if not root.is_dir():
        print(f"package directory is missing: {root}", file=sys.stderr)
        return 1

    entries = [root, *sorted(root.rglob("*"), key=lambda item: item.as_posix())]
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=1767225600) as compressed:
            with tarfile.open(mode="w", fileobj=compressed, format=tarfile.PAX_FORMAT) as archive:
                for entry in entries:
                    relative = entry.relative_to(stage).as_posix()
                    info = archive.gettarinfo(str(entry), arcname=relative)
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = 1767225600
                    if info.isfile():
                        with entry.open("rb") as source:
                            archive.addfile(info, source)
                    else:
                        archive.addfile(info)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
