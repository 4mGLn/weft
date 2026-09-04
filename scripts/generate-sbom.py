#!/usr/bin/env python3
"""Generate a deterministic CycloneDX inventory from Cargo's locked graph."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import uuid
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} OUTPUT", file=sys.stderr)
        return 2

    root = Path(__file__).resolve().parents[1]
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            cwd=root,
            text=True,
        )
    )
    lock_digest = hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest()
    packages = {package["id"]: package for package in metadata["packages"]}

    def reference(package: dict[str, object]) -> str:
        return f"pkg:cargo/{package['name']}@{package['version']}"

    components = []
    for package in sorted(packages.values(), key=lambda value: (value["name"], value["version"])):
        component = {
            "type": "application" if package["name"] == "weft-cli" else "library",
            "bom-ref": reference(package),
            "name": package["name"],
            "version": package["version"],
            "purl": reference(package),
        }
        if package.get("license"):
            component["licenses"] = [{"expression": package["license"]}]
        components.append(component)

    resolve = metadata["resolve"] or {"nodes": []}
    dependencies = []
    for node in sorted(resolve["nodes"], key=lambda value: reference(packages[value["id"]])):
        dependencies.append(
            {
                "ref": reference(packages[node["id"]]),
                "dependsOn": sorted(reference(packages[item]) for item in node["dependencies"]),
            }
        )

    document = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, lock_digest)}",
        "version": 1,
        "metadata": {"component": {"type": "application", "name": "weft", "version": packages[next(item for item in packages if packages[item]["name"] == "weft-cli")]["version"]}},
        "components": components,
        "dependencies": dependencies,
    }
    output = root / sys.argv[1]
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
