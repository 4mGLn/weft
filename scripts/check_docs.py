#!/usr/bin/env python3
"""Validate local Markdown links without third-party dependencies."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(sys.argv[1]).resolve() if len(sys.argv) == 2 else Path(__file__).resolve().parent.parent
LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)]+)\)")


def main() -> int:
    if len(sys.argv) > 2:
        print(f"usage: {sys.argv[0]} [ROOT]", file=sys.stderr)
        return 2
    failures: list[str] = []
    for document in sorted(ROOT.rglob("*.md")):
        if any(part.startswith(".") and part not in {".agent", ".agents", ".github"} for part in document.relative_to(ROOT).parts):
            continue
        text = document.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), 1):
            for raw in LINK.findall(line):
                target = raw.split(maxsplit=1)[0].strip("<>")
                if not target or target.startswith(("#", "http://", "https://", "mailto:")):
                    continue
                path_text = unquote(target.split("#", 1)[0])
                target_path = (document.parent / path_text).resolve()
                if not target_path.exists():
                    failures.append(f"{document.relative_to(ROOT)}:{line_number}: missing {target}")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print("documentation links: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
