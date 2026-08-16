# Third-Party Notices — Foundation Inventory

P2P Desk uses third-party npm packages and Rust crates. The Gate 1 component-level inventory and reported license expressions are provided in:

- `evidence/gate_01_dependency_inventory.csv`
- `evidence/gate_01_dependency_summary.json`
- `evidence/gate_01_sbom.spdx.json`

The initial metadata scan found no unknown license and no AGPL/GPL-3 candidate expression. Gate 4's persistence-specific inventory and advisory result are recorded in `evidence/gate_04_persistence_dependency_inventory.json` and `evidence/gate_04_persistence_cargo_audit.json`; that locked graph also has no unknown-license or strong-copyleft candidate and no matched RustSec vulnerability. See `docs/dependency-review.md` for scope and advisory disposition.

This source notice tracks gate evidence. The final portable release must include the release-specific notices, applicable full license texts, final SBOM, manifest, and checksum after the actual Windows dependency graph and EXE are built.
