# Persistence, retention, migration, and backup core

## Boundary

`crates/p2p-persistence` is the trusted local storage boundary for P2P Desk. The Tauri shell opens it under the operating-system-resolved `P2P Desk` local-data root. The webview has no SQLite, filesystem, backup, or restore capability.

The core accepts a provider `Acquisition` only after both user-intent sides are complete. It rechecks pair, target, quality counts, side mapping, page sequence, timestamps, freshness, and side summaries before opening a write transaction. A single `BEGIN IMMEDIATE` transaction writes the context, complete header, page receipts, both-side membership, content-addressed ad versions, payments, and summaries. Faults before commit leave none of those rows visible.

## Privacy and exact values

- Source advertisement, merchant, and request identifiers are transformed with HMAC-SHA-256 under a random 32-byte per-install identity key before storage.
- The key is created with operating-system entropy, stored separately from the database, and restricted to owner read/write permissions on Unix-family systems. It is included in validated backups so restored pseudonyms remain stable.
- Public merchant nicknames and provider response bodies have no persistence field and are never part of a content hash.
- Normalized ad versions are SHA-256 content addressed and reused across snapshot membership.
- Exact decimals are canonical strings in SQLite `TEXT` columns. The schema has no `REAL` column, and startup/restore semantic checks parse every stored decimal through `ExactDecimal`.

## SQLite policy

- Bundled SQLite through exact-pinned `rusqlite`.
- Foreign keys enabled.
- WAL journal, full synchronous durability, bounded 5-second busy timeout, and truncate checkpoints for quiescent backup/restore.
- Incremental auto-vacuum for newly created databases.
- Default maximum page count derived from the managed 2 GiB cap.
- `quick_check` on open; full `integrity_check` for backup restore and explicit diagnostics.
- Corrupt, newer-schema, locked, and full-disk failures remain explicit. No silent reset or history-as-live fallback occurs.

## Retention

Default tiers are:

| Layer                                             | Default |
| ------------------------------------------------- | ------: |
| Change-deduplicated ad detail/membership          |  7 days |
| Complete snapshot headers, summaries, and quality | 90 days |
| Hourly/daily rollups                              | 2 years |
| Managed database/history cap                      |   2 GiB |

Maintenance removes expired complete membership first, then expired complete snapshots, then expired rollups. Orphaned ad versions are removed only after membership deletion. Early cap pressure removes the oldest complete snapshot's detail at one atomic boundary at a time and protects the newest complete snapshot. Settings, costs, pair catalog, annotations, named views, and external backups are not routine-retention targets. Every effective retention/cap action is indexed in `retention_events`.

## Maintenance scheduling boundary

The Gate 4 core exposes one validated `prune_retention` operation for post-commit and low-priority maintenance and tests its tier/cap behavior. Gate 5 owns lifecycle scheduling: it must call this operation after each successful refresh commit and during low-priority maintenance, surface failures without reclassifying an already committed snapshot, and never overlap it with an active publication. Keeping scheduling out of the persistence transaction avoids reporting a durable committed snapshot as an acquisition failure merely because later maintenance failed.

## Cost versions and local-state foundations

Cost identity is pair + exact route + Buy/Sell leg + payment method. Every edit inserts an immutable content-addressed version with effective dates, fixed fiat, percentage fiat, fixed asset, min/max charge, fixed/percentage buffer, label, source, and note. SQL `NULL` means unknown; canonical text `"0"` means explicit zero.

Versioned foundations also exist for validated pairs/payments, settings, chart annotations, named views, report audit metadata, and redacted diagnostic metadata.

## Backup and restore

A backup is an atomically persisted, stored-compression ZIP containing exactly:

1. `database.sqlite3` — a consistent SQLite online backup;
2. `identity.key` — the 32-byte pseudonym key;
3. `manifest.json` — product, format, schema/runtime versions, included domains, byte sizes, and SHA-256 hashes.

Restore rejects duplicate/unexpected members, oversized members, hash/size mismatch, invalid identity keys, newer schemas, manifest/database version disagreement, integrity failure, semantic decimal/schema failure, and insufficient free space. Before replacement it checkpoints and quiesces the current connection and creates a safety backup. Database and key are staged in the data filesystem, replaced with rollback copies under a durably synchronized restore marker, reopened, integrity checked, migrated if needed, and rolled back on any failure. Startup detects an interrupted marked swap and restores the prior database/key pair before opening SQLite. Existing-database migrations create a consistent automatic backup before every pending migration; the newest five automatic migration/restore-safety backups are retained.

## Independent destructive scopes

The core exposes separate confirmed-operation foundations for:

- history;
- annotations and named views;
- settings;
- logs;
- all local data.

Each database clear is one immediate transaction. Filesystem clears operate only within known app-managed directories and remove symlinks as entries rather than traversing them.
