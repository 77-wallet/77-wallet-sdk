# Ordinary catalog incremental sync

## Confirmed scope and prior history

User rejects repeated full refresh and a one-time historical backfill. Previously missed development metadata that never receives a later backend update is intentionally not repaired. Future backend changes must become eligible through updateTime; new-install catalog initialization remains necessary. The separate DAO timestamp fix is committed/pushed as37eb285b, not part of the uncommitted synchronization change.

History:0713fc83a (2025-07-25) introduced creation-time incremental selection; bc6d918e5 (2025-07-28) temporarily used no creation filter with a comment about avoiding missed updates after version changes;7401d477a (2025-08-05) restored creation filtering above500 local rows. The count was an initialization heuristic, not a successful-sync time. No measured performance motivation was found.

## Starting point and independent queries

The old creation filter was the latest non-custom local row's created_at, formatted to seconds. It was not last synchronization time. The candidate uses the same latest creation timestamp as both starting cursors for an existing catalog, never max local updated_at and never zero/epoch to force history replay. It does not retroactively query before that starting point except a fixed two-second boundary overlap.

State is a private JSON value ordinary_catalog_progress_v1 in the existing core config table (type1). Missing state captures and persists the starting cursor BEFORE network/metadata changes. That freezes the retry window if a later stage fails after some metadata commits. With no server-created row (no row or bundled epoch default timestamps), a null cursor records normal unfinished new-install initialization. Such a new install still uses the original unfiltered endpoint until its initialization succeeds. Existing catalogs never receive a historical bootstrap reset.

After initialization, each invocation calls existing fetch_all_tokens(None, update_cursor-2s) and separately fetch_all_tokens(create_cursor-2s, None). Each has its own page0,size500 traversal. Both time fields are never combined: nine native Rust probes were consistent with AND/intersection. Independent creation traversal preserves new-coin selection without assuming every new coin's update timestamp establishes creation eligibility.

Returned timestamps are parsed with the existing strict UTC parser. Cursors advance monotonically only from valid returned server creation/update times, separately; empty results do not advance them. Update-descending ordering puts the newest overlapping server version first for the existing identity deduplication. Equal-time versions still use existing first-seen behavior, with update-query records first.

## Completion, failure and concurrency

Both traversals finish before metadata upsert. Network/page/cancellation/timestamp errors prevent that upsert and retain the starting/saved cursor. Metadata/precision commit in the existing repository transaction. Default restoration, swappable updates and ordinary default-asset reconciliation still run. Candidate progress is saved only at the end of a successful service invocation, after those steps. Progress-save failure safely causes the old window to replay; no state can exclude uncommitted metadata. Persisting the starting point is initialization, not success advancement. It adds a state write even when the first request fails.

A Context-local ordinary_catalog_sync_lock covers cursor read, both queries, metadata and completion, so a second invocation reads progress after the first completes. Cancellation releases the lock. This does not serialize other processes/independent Contexts or the separate price flow. Existing pre-network empty-token cleanup remains outside the metadata transaction, so the whole service is not zero-write on failure. Frontend notification failure retains the existing warning-only behavior after durable asset reconciliation.

The two-second overlap is for formatted-second equality and short boundary replay, not an arbitrary late-event guarantee or a history scan. Server precision/timezone and snapshot/tie ordering contracts remain unconfirmed. Existing concurrent offset-page omissions may occur; overlaps and deduplication do not prove their absence. Changes backdated before the saved window, or never-updated historical omissions, may stay missed by the accepted scope. No client wall-clock watermark or fixed upper-bound claim is added.

## Cost and acceptance

There is no change to trigger cadence. Each ordinary incremental invocation makes at least two directory requests; with U updated and C created-window rows, it requests max(1,ceil(U/500))+max(1,ceil(C/500)) pages. It collects O(U+C), then deduplicates and writes returned identities in one metadata transaction, plus existing precision coordination/default/assets stages. Repeated boundary rows and rows present in both traversals are transferred again. It does not transfer/write all catalog rows every cold start. New-install initialization still has the observed1286-row/three-page cost, not a supported catalog bound or performance certification.

Read-only sf-wallet-app source HEADdf33195d: startup SDK/device initialization queues PullHotCoins; an extra connectivity-ready syncHotCoins call is gated by a persistent requested flag. No periodic hot-coin timer/reset caller was found. Runtime cadence/latency was not measured, App was not modified, and candidate SDK is not published or installed.

Offline fake-backend/SQLite regression evidence must be tied to this increment-specific version, not earlier full-replay tests. Actual App cursor, future backend mutation to device display, real restart and concurrent backend paging remain unverified. No second-issue commit/push/merge/plugin release is authorized before the user reviews tests.

Rust persistence tests close all old SQLite pools and replace Context on identical files, and run initialize/resume pairs in separate test processes. The first process exits before the second opens the files. Distinct successful watermarks and a frozen first-request-failure baseline drive exact incremental cutoffs despite a poisoned local update timestamp. This is isolated Rust recovery coverage; real App restart remains unverified.
