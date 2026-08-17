# Lifecycle, Settings, and Refresh Orchestration

## Trusted ownership

Gate 5 places startup, settings, market context, refresh scheduling, cancellation, state transitions, filtering, exact calculation preparation, and publication orchestration in Rust. `crates/p2p-lifecycle` owns the typed state machine. `src-tauri/src/lifecycle_commands.rs` is the narrow native adapter connecting that state machine to the live provider and atomic persistence store.

The frontend receives typed views, the non-secret current request ID for copyable diagnostics, and aggregate acquisition progress. It cannot publish values, access the network directly, read history as live data, or substitute demo, cache, Agent, or prior snapshot values.

## Startup and restoration

Startup proceeds through loading settings, restoring context, loading catalog foundations, and ready. The first run validates and persists one versioned lifecycle record containing:

- Auto-refresh ON;
- 20-second interval;
- draft and applied USDT/EGP context;
- fiat amount 10,000;
- neutral eligibility filters;
- 40-result target; and
- no prior successful timestamp.

The interval is an integer from 10 through 3,600 seconds. A stored record must have the supported version and pass the same settings/context validation used for new input. Malformed, unsupported, or semantically invalid restored state becomes an actionable `invalid-restored-state` error. It is never silently replaced by defaults. Draft edits, settings edits, Apply, and Refresh remain blocked in that state so compiled defaults cannot become an implicit live context. A separate reset command applies validated first-run defaults only after an explicit user action.

Draft and applied contexts are stored separately. Editing draft values sets `unappliedChanges`; Apply atomically persists both values before starting a refresh against the newly applied context. Persistent mutations are prepared on a validated replacement controller, durably saved, and only then swapped into live memory.

## Scheduling and freshness

A Rust-owned task evaluates automatic refresh once per second while the application process is open, including while its window is minimized. It uses monotonic timer ticks with missed-tick skipping, then evaluates deadlines against wall-clock observation times. This avoids overlapping catch-up bursts after sleep.

The first due automatic operation is a typed startup refresh. Later countdowns start only after a successful complete commit. Automatic refresh never overlaps an active operation. Normal, retry, and wake deadlines are rechecked inside the same active-operation serialization boundary used to start refreshes, so a concurrent settings change cannot launch an obsolete automatic request. Timed provider circuits pause automatic attempts until closed; transient provider failures use the configured interval as an internal retry throttle. A persistent contract circuit requires its explicit provider diagnostic path.

On wake, the explicit wake command refreshes when observation age exceeds the interval. Browser connectivity changes can call the offline command: going offline atomically excludes a concurrent refresh start, cancels an already active provider token, and removes live values; reconnecting re-arms age-relative scheduling. Process exit stops all scheduling—there is no service, task, or autorun component.

Freshness becomes stale after `max(60 seconds, 2 × refresh interval)`. Stale values and clock anomalies are represented as error views, not live views. Clock movement backwards also makes the next refresh due.

## Refresh graph

Each refresh is one serialized graph:

1. queue and remove prior live values from the lifecycle view;
2. acquire alternating Buy and Sell provider pages with cancellation;
3. validate pair, side, contract, and local eligibility;
4. continue paging until each side reaches the eligible target or trustworthy provider exhaustion;
5. persist the provider-validated pair as enabled with observed payment methods and explicit provider-unspecified precision metadata;
6. deterministically rank eligible results using exact domain calculations;
7. distinguish provider-empty, local-no-match, cancelled, and typed failure states;
8. validate the complete prepared two-side result again;
9. publish acquisition, applied context, timestamps, quality, memberships, and summaries in one SQLite transaction; and
10. run retention pruning after commit.

Low-priority retention maintenance also runs during startup. A startup or post-commit pruning problem is recorded as a maintenance warning and never changes a valid restoration or committed acquisition into a provider failure.

Only `PreparedAcquisition::Publish` can reach `publish_complete_snapshot`. Partial, ineligible, mismatched, stale, failed, or cancelled acquisitions cannot publish. A post-commit pruning or lifecycle-metadata warning cannot reclassify the already committed acquisition as failed.

## Typed frontend boundary

The lifecycle contract is mirrored in `app/src/ipc/lifecycle-contracts.ts`; invocations are centralized in `app/src/ipc/lifecycle-client.ts`. The approved UI gate will consume this boundary rather than implement independent scheduling or fallback behavior.
