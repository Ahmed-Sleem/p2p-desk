#!/usr/bin/env python3
"""Generate a deterministic Gate 1 SPDX 2.3 JSON inventory from the reviewed CSV."""
from __future__ import annotations

import csv
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "evidence/gate_01_dependency_inventory.csv"
OUTPUT = ROOT / "evidence/gate_01_sbom.spdx.json"


def spdx_id(ecosystem: str, name: str, version: str) -> str:
    digest = hashlib.sha256(f"{ecosystem}:{name}:{version}".encode()).hexdigest()[:12]
    stem = re.sub(r"[^A-Za-z0-9.-]", "-", f"{ecosystem}-{name}-{version}")[:120]
    return f"SPDXRef-{stem}-{digest}"


with INVENTORY.open(newline="", encoding="utf-8") as handle:
    components = list(csv.DictReader(handle))

root_id = "SPDXRef-P2PDesk"
packages = [
    {
        "name": "P2P Desk",
        "SPDXID": root_id,
        "versionInfo": "0.1.0",
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": "NOASSERTION",
        "copyrightText": "NOASSERTION",
        "primaryPackagePurpose": "APPLICATION",
    }
]
relationships = []
for component in components:
    ecosystem = component["ecosystem"]
    name = component["name"]
    version = component["version"]
    identifier = spdx_id(ecosystem, name, version)
    purl_name = name.replace("@", "%40")
    packages.append(
        {
            "name": name,
            "SPDXID": identifier,
            "versionInfo": version,
            "downloadLocation": "NOASSERTION",
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "NOASSERTION",
            "copyrightText": "NOASSERTION",
            "comment": f"Registry metadata license expression: {component['license']}",
            "externalRefs": [
                {
                    "referenceCategory": "PACKAGE-MANAGER",
                    "referenceType": "purl",
                    "referenceLocator": f"pkg:{ecosystem}/{purl_name}@{version}",
                }
            ],
        }
    )
    relationships.append(
        {
            "spdxElementId": root_id,
            "relationshipType": "DEPENDS_ON",
            "relatedSpdxElement": identifier,
        }
    )

document = {
    "spdxVersion": "SPDX-2.3",
    "dataLicense": "CC0-1.0",
    "SPDXID": "SPDXRef-DOCUMENT",
    "name": "P2P-Desk-Gate-1-SBOM",
    "documentNamespace": "https://arena.ai/p2p-desk/spdx/gate-01-2026-08-14",
    "creationInfo": {
        "created": "2026-08-14T00:00:00Z",
        "creators": ["Tool: P2P Desk scripts/generate_sbom.py"],
    },
    "documentDescribes": [root_id],
    "packages": packages,
    "relationships": relationships,
}
OUTPUT.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
print(f"wrote {OUTPUT} with {len(packages)} packages")
