#!/usr/bin/env python3
"""Create a deterministic direct/transitive dependency and license inventory."""
from __future__ import annotations

import csv
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "evidence"
EVIDENCE.mkdir(exist_ok=True)

cargo = json.loads(subprocess.check_output([
    "cargo", "metadata", "--format-version", "1", "--locked",
    "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"),
], text=True))
rows: list[dict[str, str]] = []
for package in cargo["packages"]:
    if package["name"] == "p2p-desk":
        continue
    rows.append({
        "ecosystem": "cargo",
        "name": package["name"],
        "version": package["version"],
        "license": package.get("license") or "UNKNOWN",
        "source": package.get("source") or "workspace/path",
    })

for package_json in sorted((ROOT / "node_modules").rglob("package.json")):
    if any(part.startswith(".") for part in package_json.relative_to(ROOT / "node_modules").parts):
        continue
    try:
        package = json.loads(package_json.read_text())
    except (UnicodeDecodeError, json.JSONDecodeError):
        continue
    name = package.get("name")
    version = package.get("version")
    if not isinstance(name, str) or not isinstance(version, str):
        continue
    license_value = package.get("license", "UNKNOWN")
    if isinstance(license_value, dict):
        license_value = license_value.get("type", "UNKNOWN")
    rows.append({
        "ecosystem": "npm",
        "name": name,
        "version": version,
        "license": str(license_value),
        "source": "package-lock.json",
    })

unique = {(row["ecosystem"], row["name"], row["version"]): row for row in rows}
rows = sorted(unique.values(), key=lambda row: (row["ecosystem"], row["name"], row["version"]))

with (EVIDENCE / "gate_01_dependency_inventory.csv").open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=["ecosystem", "name", "version", "license", "source"])
    writer.writeheader()
    writer.writerows(rows)

summary = {
    "components": len(rows),
    "cargo": sum(row["ecosystem"] == "cargo" for row in rows),
    "npm": sum(row["ecosystem"] == "npm" for row in rows),
    "unknownLicense": [row for row in rows if row["license"] == "UNKNOWN"],
    "strongCopyleftCandidates": [row for row in rows if any(token in row["license"].upper() for token in ["AGPL", "GPL-3", "GPLV3"])],
}
(EVIDENCE / "gate_01_dependency_summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
