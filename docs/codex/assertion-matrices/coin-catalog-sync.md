# Ordinary coin timestamps and incremental sync

The independent timestamp correction is committed as37eb285b. Its four temporary SQLite regressions cover distinct insert timestamps, conflict defaults/update time, rollback/retry and repeated legacy-row correction; use cargo test -p wallet-database --offline --locked --lib catalog_metadata_tests -- --test-threads=1.

## Uncommitted incremental candidate

| Scenario | Required invariant | Fake-backend integration regression |
| --- | --- | --- |
| Existing501-row catalog, future update to old USDT plus newly created USDC; repeat | Separate bounded filters from latest creation baseline, repaired default/time, no duplicate identities, overlap includes last successful second | ordinary_catalog_incremental_fetches_future_old_updates_and_new_coins_above_500 |
| Historical row not updated again | No full/backfill request; old metadata remains until a future update arrives | ordinary_catalog_does_not_backfill_history_but_fetches_later_updates |
| Last update page fails/cancels | No partial metadata/assets, frozen starting cursor retained; retry begins page0 | ordinary_default_assets_failed_later_page_writes_no_partial_assets_or_coins; ordinary_default_assets_cancelled_last_page_does_not_write_or_notify |
| First request or creation traversal fails | Both traversals must complete before upsert/progress advancement | ordinary_catalog_request_and_creation_route_failures_do_not_advance |
| Metadata transaction or progress-save fails | Metadata rollback or safe replay; cursor never advances past failed work | ordinary_catalog_metadata_and_progress_failures_replay_old_window |
| Two simultaneous invocations | Second query waits for first completion and reads the resulting cursor | ordinary_catalog_serializes_overlapping_syncs_before_cursor_read |
| Same coin returned with different versions from both routes | Newer update version wins, one row persists, creation/update watermarks stay independent | ordinary_catalog_overlapping_queries_keep_newest_server_version |
| Invalid server timestamp then retry | No metadata write or progress advancement until valid response | ordinary_catalog_invalid_server_time_preserves_cursor_until_retry |
| New-install initialization paused at last page | Existing unfiltered initialization retained; no metadata/assets until all pages succeed | ordinary_default_assets_wait_for_last_page_then_fill_and_repeat_safely |
| Later completion fails after metadata | Null new-install starting state retained; retry completes initialization/reconciliation | ordinary_default_assets_failed_catalog_completion_defers_fill_until_retry |

Run correct default features: cargo test -p wallet-api --offline --locked --test mod ordinary_default_assets -- --test-threads=1. Tests use only temporary SQLite and localhost fake backend. They assert actual requests, persisted coin/default timestamps, assets, progress and failure/retry behavior. Existing cleanup before network and notification warning semantics are not redefined. Server offset-page drift, long-backdated updates, cross-process synchronization, real device acceptance and catalog-scale timing remain unverified; two-second overlap is not an absolute completeness claim.

| Persistence scenario | Evidence |
| --- | --- |
| Close all old SQLite read/write pools and replace Context on same files | `ordinary_catalog_rebuilt_context_restores_distinct_successful_watermarks`: distinct persisted watermarks drive exact independent cutoffs |
| Frozen initial state survives replacement after network failure | `ordinary_catalog_rebuilt_context_retries_frozen_baseline_after_network_failure`: initial window reused despite poisoned local updated_at |
| Successful initialization is not repeated after replacement | `ordinary_catalog_rebuilt_context_avoids_reinitialization_and_fetches_future_updates`: incremental requests apply later updates |
| Old Rust process exits before a new process opens identical files | `ordinary_catalog_process_exit_restores_persisted_watermarks`: success and first-request-failure pairs, child execution count and exact cutoffs asserted |

Context replacement keeps the original static allocation leaked to avoid unsafe reclamation; all old DB pools are explicitly closed. Separate-process coverage supplies process teardown evidence. Neither is a real App restart test.

## Product chain naming / native symbol rename (2026-10-26)

Backend remains authoritative; local chain and native-coin defaults follow the
user-provided 14-chain table (TON native symbol GRAM). No chain enablement or
contract-token alias change. Existing users converge after successful catalog
sync; offline backend-owned names are not forcibly overwritten.

| Scenario | Evidence / invariant |
| --- | --- |
| New-install defaults | `supported_chain_display_names_match_product_catalog`, `native_symbols_match_product_catalog_in_both_networks`: all 14 chain names/native symbols, both coin networks |
| Ordinary native TON → GRAM, replay | `native_catalog_rename_preserves_holdings_and_replays_without_aliases`: one native coin identity; visible/hidden native assets renamed; balances/status/observation time unchanged; contracted token and other chain untouched |
| Existing conflicting asset aliases | `native_catalog_rename_conflicting_assets_roll_back_without_merging_balances`: error, old coin and both cached amounts preserved atomically; no automatic balance merge |
| Conflicting native symbols in one batch | `native_catalog_rename_conflicting_batch_does_not_create_symbol_aliases`: reject without partial catalog writes |
| API startup/incremental native metadata | `api_native_catalog_rename_reuses_identity_and_preserves_hidden_balance`: reuse chain+empty-contract identity, repeat without duplicate, hidden balance/time preserved. Base fixture does not expand supported chains. |
| Backend chain rename + replay | `backend_catalog_renames_existing_chains_without_replacing_bindings`: ordinary TON/GRAM and API BNB Chain persisted, manual node IDs preserved. Isolated fixture stops at missing task handles after DB writes; does not verify task delivery or real backend. |

Native label writes reuse `CoinRepo::upsert_multi_coin`'s transaction and existing
precision repair; transaction cancellation/failure cannot commit a partial rename.
Price-only update callers and contract metadata paths are unchanged. No new
network request, worker, account scan or synchronization cursor is introduced.
Cross-process/delayed server-version ordering retains existing behavior; a later
old backend response may restore an old name/symbol, so backend rollout must agree
with the approved catalog. Conflicting legacy native aliases require separate
repair rather than choosing a cached balance here. Live App/backend rollout and
physical-device offline/restart acceptance are not covered by these tests.

## WALLET-2981 metadata access paths (2026-11-05)

Preserve existing API catalog/asset label propagation and one batch transaction/writer guard. The EVM comparison remains NOCASE (including legacy stored spellings), non-EVM remains case-sensitive. Scalar source lookup, EXISTS/difference predicate and all four bindings are unchanged. No skipping replays based only on unchanged catalog metadata: stale asset labels must still converge. Two additive non-unique chain/token-NOCASE indexes plus EVM-only INDEXED BY guarantee the measured access paths. No historical migration edited, no balance/visibility/precision/timestamp or public FFI change.

| Scenario | Evidence / invariant |
| --- | --- |
| Skewed statistics, native and absent contract assets | `coin_metadata_access_uses_exact_token_indexes_after_analyze`: production SQL EXPLAIN uses full chain+token keys for assets and correlated catalog; no table scan |
| Existing populated schema upgrade | `coin_metadata_access_additive_migration_preserves_existing_rows`: additive migration builds both indexes with legacy spelling/labels/balance/status/precision/times unchanged |
| Missing catalog | `coin_metadata_access_missing_catalog_leaves_existing_asset_intact`: no unintended label/null/balance/status write |
| Unchanged catalog with stale cached labels | `api_coin_metadata_replay_repairs_drift_when_catalog_is_unchanged`: visible and hidden assets converge; other scopes, values/times unchanged |
| Multi-coin injected failure | `api_coin_metadata_batch_failure_preserves_catalog_and_assets_atomically`: catalog updates/new insertion and asset writes all roll back |
| Existing rename, incremental/price-only update, native/non-EVM, failure | Existing five `api_coin_metadata_rename_*` tests retained |
| Real repository at scale | Explicit ignored `api_coin_metadata_scale_500k`:500000 api_account rows/distinct addresses and500000 assets,512 synthetic same-chain coins; migrated SQLite/ApiCoinRepo, concurrent ApiWalletRepo writer, true rename/replay plus balance/status/precision/multisig/timestamp/count invariants |

Recorded single host-debug measurements with SDK SQLite3.46: original repo replay85.826s/wallet wait85.822s; fixed repo replay0.698s/wallet wait0.692s. Building new indexes on populated fixture0.426s, real500k-label rename5.425s, replay0.658s. Setup seeding16.748s original vs18.164s with indexes (not a controlled write-throughput benchmark); added index pages approximately6.23MiB for this synthetic distribution. Both migration and actual updates still take real time. These are synthetic host results, not QA-device/iOS whole-wallet creation timings; baseline also overlapped a scoped compile, no statistical/performance SLA claim.

Run actual crate commands with nightly2026-04-17/offline locked: `cargo test -p wallet-database --lib --no-default-features coin_metadata`; explicit scale: same command filter `api_coin_metadata_scale_500k -- --ignored --nocapture`. Original baseline is captured before enabling hints/migration with `METADATA_SCALE_BASELINE=1`. Related API EVM identity/token spelling/catalog repo scoped regression filters also apply. Full suite, real backend/new iOS package and affected QA device remain unverified; retain App diagnostic marker for QA follow-up.

## API asset canonical labels (2026-11-05 candidate)

This phase supersedes the historical beta350 asset-cache synchronization implementation above. Old index measurements remain release history, not the current write contract.

| Contract | Actual SQLite coverage |
| --- | --- |
| Canonical name/symbol, unchanged asset precision/protocol and cached rows | catalog_labels_read_tests, catalog_labels_refresh_tests |
| Missing catalog fallback; disabled catalog and hidden asset retain caller visibility | catalog_labels_visibility_tests |
| Empty labels authoritative; EVM mixed case, non-EVM exact identity, no row multiplication | catalog_labels_policy_tests |
| Additive migration, EVM partitions match uniqueness guard, scoped indexed lookup | catalog_labels_schema_tests |
| Batch/incremental rename succeeds while asset-label writes are forbidden | catalog_labels_write_tests, metadata_rename_tests |
| Catalog failure rolls back batch updates/new inserts and incremental update | metadata_rename_tests |
| 500000 address read/write invariants and forbidden asset label updates | catalog_labels_scale_tests, metadata_scale_tests (explicit ignored runs) |

Raw refresh readers retain missing-catalog assets and now expose current labels. Display/status filters and symbol grouping use current catalog labels. Existing v2 aggregate reuses its catalog join. Label-independent sparse totals retain raw indexed asset access. Three affected tuple-key builders use existing QueryBuilder.push_values to fix previously invalid SQL without changing key parsing or parameter binding. Public DTO shapes and historical notification symbols are unchanged.

Final host debug evidence (bundled SQLite3.46, actual migrated context/public repositories, synthetic data): 500000 addresses/assets and512 coins; selected20 rows3.008ms, address refresh0.674ms, full-account summary3.208s, full-token refresh250000 visible rows2.526s. Catalog rename16.064ms; all500000 compatibility rows untouched and no non-label mismatch. Separate catalog probe: unchanged replay16.449ms, rename15.177ms, replay14.041ms; writer wait14.962ms. Single host runs, not iOS/App latency guarantees. Full refresh retains existing large result/memory behavior.

Validation logs: target/wallet-2981-cold-start/diagnostic/catalog-labels-{dao-final,coin-regression,assets-regression,api-tests,scale-reads-final,scale-catalog}.log in the primary workspace. Scoped DB tests58passed plus2 explicit scale probes; API asset consumer tests10passed. Full suite, iOS native package, device reproduction and live backend are not covered. Current phase remains uncommitted pending review/commit authorization.

## Historical physical-removal feasibility probe (2026-11-05, superseded below)

User requested database-only validation of old-library upgrade and500000 addresses after actual column removal. No production migration or writer behavior is changed. The test-only candidate drops the current view and two symbol-containing indexes, physically drops both columns in one transaction and recreates the same catalog joins with NULL instead of the now-unavailable fallback. This NULL branch is a probe, not an approved product policy. Old-schema fixture represents beta350 (all prior migrations, populated assets, latest view migration absent); actual current migrator upgrades it before candidate removal.

| Scenario | Evidence / expected result |
| --- | --- |
| Direct DROP name with current view | catalog_labels_drop_probe_old_upgrade_and_direct_drop_rollback: view dependency blocks DROP, rollback leaves prior data readable |
| DROP symbol after removing view/name but keeping indexes | catalog_labels_drop_probe_symbol_index_failure_rolls_back_partial_schema_change: symbol index blocks DROP; explicit transaction rollback restores columns, data and view |
| Prepared drop with matching catalog | actual DAO identity read, balance update, status hide/show work; precision/protocol/timestamps preserved; integrity_check=ok |
| Current production writers against dropped schema | catalog_labels_drop_probe_current_writers_reject_removed_columns: single upsert, bulk upsert, missing initialization and balance upsert all fail with no column named name; no inserted rows or balance mutation |
| Missing catalog previously had fallback | catalog_labels_drop_probe_missing_catalog_loses_legacy_labels: old symbol OLD becomes NULL in view and empty string in current refresh DTO; original compatibility information is unrecoverable |
| 500000 existing assets | catalog_labels_drop_probe_scale_500k: physical removal plus existing public repository reads/catalog writes; exact bidirectional EXCEPT of every remaining field, integrity check and fresh-connection migration/read verification |

The expected-error tests passing proves incompatibility is reproduced, not that physical removal is safe. Current beta351 therefore cannot drop these columns without writer changes, dependency cleanup and an approved missing-catalog policy. No deployed database/QA device is touched, and no package is released for this probe.

Actual500k host result: seed16.799s; test-only physical removal plus integrity verification9.989s (includes integrity_check, not isolated DDL timing). Selected20 reads1.101ms; address refresh0.624ms; full summary1.388s; full token refresh250000rows1.843s; catalog rename16.077ms. All500000 identities and every retained column match the pre-drop snapshot in both EXCEPT directions; fresh-connection migration/read succeeds and columns remain absent. These are single host debug runs, not iOS performance predictions or evidence that missing writers are fixed. Logs: primary workspace target/wallet-2981-cold-start/diagnostic/catalog-labels-drop-{small,final,regression}.log.

Final scoped DAO regression:34passed,2ignored (both scale probes explicit-only); dropped-schema500k probe passed separately. Scoped Rust formatting and diff check pass. No production fixes, final migration or mobile tests are claimed.

## Production physical label removal (2026-11-05 candidate)

User approved actual removal after recovery, and explicitly chose newest updated time (fallback created time), with larger asset ID only for equal instants. This section supersedes the beta351 compatibility-column and throwaway-probe contracts above.

Production scope is one new migration plus four INSERT paths in ApiAssetsDao. Single upsert still changes only status/multisig/time on conflict; bulk upsert retains the same policy; missing initialization remains DO NOTHING; balance upsert changes only balance/time. Public input/output DTO labels remain; storage reads resolve labels from api_coin.

Migration20261105020000 runs in the existing SQLx migration transaction. It recovers missing chain/token identities without overriding present catalog rows (including disabled/empty-label catalog), preserves EVM identity normalization and non-EVM case sensitivity, then drops only the dependent label view/two symbol indexes/two columns and recreates the catalog-only view. Identity/covering/nonzero indexes remain. Recovery copies name/symbol/decimals/protocol from the chosen row; price defaults0, catalog status defaults1, flags default0. Epoch created/updated times prevent advancing the backend metadata watermark. Labels on every old orphan asset now resolve from the recovered catalog; previously catalog-filtered assets may become visible when their chain and own asset status are enabled. Per-address fields are never updated by recovery.

| Required contract | Current regression |
| --- | --- |
| Old351 schema, present mixed-case EVM catalog authoritative, native/sol exact missing identities recovered, other asset fields unchanged, replay safe/watermark stable | label_removal_upgrade_backfills_missing_identities_without_overwriting_catalog |
| Missing catalog INSERT fails: rollback and retry | label_removal_backfill_failure_rolls_back_and_retry_succeeds |
| Later DDL fails after recovery: catalog inserts, dropped view/index changes and migration marker roll back | label_removal_late_ddl_failure_rolls_back_backfill_columns_and_migration_marker |
| Conflicting snapshots use newest time then id, shared EVM identity yields one catalog row; visibility stays per asset | label_removal_backfill_newest_snapshot_and_id_tiebreak_are_deterministic |
| Submillisecond timestamps, whole/fraction comparison and offset-equivalent tie | label_removal_newest_timestamp_retains_submillisecond_precision_and_offset_equivalence |
| Four real public writer entry points after migration preserve insert/conflict policies and catalog | catalog_labels_removed_schema_preserves_all_four_writer_contracts |
| Old350 populated500k database missing native catalog; actual upgrade, all retained columns exact, fresh connection, read/refresh/summary/catalog replay, all4 writers | label_removal_scale_500k_upgrade_backfill_and_reads (explicit ignored) |

Read-only design review caught julianday precision loss. RED actual SQL and Rust test reproduce older-label selection for distinct microseconds; final migration compares UTC whole seconds without fractions and zero-padded nanoseconds separately. No floating date comparison remains. Migration-specific old cache tests retain explicit pre-removal schema fixtures to preserve historical index/rollback evidence; current-reader/writer fixtures use the actual latest migration. Test-only raw SQL fixtures in four API modules use the new column list. One import notification assertion now ignores existing DEBUG events on the shared channel while still asserting exactly one business event and no replay business event; production notification behavior is unchanged.

Final500k host debug run: seed15.554s, actual recovery+schema upgrade4.888s, selected20 reads3.548ms, address refresh1.677ms, full summary1.477s, token refresh250000rows1.966s, catalog replay16.414ms. All500000 identities and every remaining column match in both EXCEPT directions; integrity check and fresh-connection migration/read pass. Four post-upgrade writer paths insert4rows/balance sum36. Single host runs, not iOS timing guarantees. No actual device database, native package or deviceE2E is covered. Dropped schema is incompatible with old SDK writers; no rollback package is promised.

Evidence in primary target/wallet-2981-cold-start/diagnostic/: label-removal-writers-red/green.log, migration-red/green.log, precision-red.log, db-regression/db-final.log, scale-final.log and api-* logs. Broad DB attempt exposed a separate unchanged static bincode fixture failure in entities::multisig_queue::test; final DB run explicitly excludes it (693passed,6ignored,1filtered). Broader API domain attempt:16loopback-bind failures under sandbox and2no-feature profile assertions (unknown vs test); these are outside production/schema changes. Affected API asset/coin/visibility/import filters are validated separately; no full API-suite pass is claimed.

Final affected API results on final source: assets21passed, coin6passed, chain visibility3passed, import notification7passed (37total); final actual API compilation succeeded. Scoped formatting/diff checks pass. No broad API-suite or fullDB-suite pass is claimed for the excluded/environment failures above.

Supplemental actual simulator backup verification: explicit ignored test label_removal_saved_old_database_copy_upgrade_preserves_every_asset_field takes WALLET_LABEL_REMOVAL_OLD_DB, opens source read-only and uses VACUUM INTO for a consistent disposable copy. Historical migration checksums match current files. Saved old schema last version20261102020000,272929 account addresses/3074656 assets. Actual SQLx upgrade16.107s; every remaining asset field has zero bidirectional EXCEPT differences, row/view cardinality unchanged, columns absent and integrity_check=ok. Entire test171.97s includes copying/snapshotting/full comparisons/integrity, not just upgrade. Log label-removal-real-old-backup.log. This is a saved simulator backup, not the tester's phone or an iOS end-to-end run. The supplemental test source was reviewed separately after the main frozen review; production code is unchanged.
