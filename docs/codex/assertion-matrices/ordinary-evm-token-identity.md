# Ordinary-wallet EVM token identity

Scope: ordinary `coin` / `assets` only. User confirmed 2026-10-20 that the
screenshot is an ordinary wallet. No API-wallet tables, App dependency changes,
emulator writes, real transactions, commit or push.

| Behavior / invariant | Evidence |
| --- | --- |
| Five chains each hold one 2-USDT cache, not a 2+0 pair | `ordinary_evm_token_identity_repairs_five_chain_snapshot_without_summing`; real SQLite migration; repeated repair leaves five pending hints |
| Canonical cache wins without adding conflicting 2 and 9 | `ordinary_evm_token_identity_conflict_retry_rollback_and_non_evm_invariants`; refresh replaces cache with 11 |
| No canonical spelling: latest timestamp is deterministic fallback | `ordinary_evm_token_identity_normalizes_singletons_and_uses_latest_fallback`; singleton keys normalize without changing balance |
| Migration and balance writes roll back atomically | `ordinary_evm_token_identity_conflict_retry_rollback_and_non_evm_invariants`; failed write and rolled-back write both preserve pending hint |
| All future writes reuse canonical EVM key; initialization cannot reset balance | `ordinary_evm_token_identity_writes_queries_and_refresh_share_one_record`; mixed-case coin/asset writes, lookup, balance update and removal |
| SQLite independently rejects case-only duplicates | `ordinary_evm_token_identity_conflict_retry_rollback_and_non_evm_invariants`; unique-index violation |
| Existing same-contract symbol aliases remain valid | `ordinary_evm_token_identity_preserves_existing_symbol_alias_dimension` plus existing default-assets alias regression |
| Native and non-EVM token keys retain existing semantics | Same database test plus `ordinary_evm_token_identity_non_evm_chain_requests_remain_case_sensitive` |
| Precision rescaling is not a remote balance observation | `ordinary_evm_token_identity_precision_repair_cannot_consume_refresh_hint`; mixed-case metadata updates decimals/balance but retains hint |
| Refresh dispatch remains scoped and bounded | `ordinary_evm_token_identity_refresh_hints_are_bounded_and_account_scoped`; other wallet/account/chain excluded, 106 eligible rows capped at 100, all reached across repeated failed batches using a durable logical dispatch order |
| Homepage and chain view agree for stale mixed-case App requests | `ordinary_evm_token_identity_homepage_and_chain_list_agree_for_legacy_request`; actual services + SQLite, five x2 vs total10, fiat equality, account4 excluded |
| Missing handles do not consume repair hints | Same service test; isolated Context reads preserve all five hints |

## Production paths

- `SqlitePoolProvider` -> core SQLx migration (one SQLite transaction) -> canonical
  cached rows + unique EVM indexes (preserving the existing symbol dimension) + durable refresh hints.
- Coin/asset DAO writes normalize via existing `AssetTokenKey` chain rules;
  precision batch uses identical normalized keys. Native/non-EVM writes unchanged.
- `AssetsService::get_account_chain_assets_v2` ->
  `AssetsDomain::request_identity_refresh` -> scoped DAO hints -> existing
  `InnerEvent::SyncAssets` -> existing RPC sync and `ordinary_balance_order` ->
  balance write clears hint atomically -> existing frontend SyncAssets notification.
- `ChainService::get_chain_assets_list` uses `matches_for_chain`; no symbol-based
  reconstruction or independent balance cache.

## Verification and limits

Commands and exact results recorded in `PLANS.md`; logs under
`target/account3-diagnosis/`. Database RED observed duplicate count10 vs expected5,
new case writes3 vs expected1. Service RED observed zero chain rows vs expected5.

Local copy of incident core DB migrated in memory: five x2 preserved, total10,
SQLite integrity_check ok; original emulator DB never opened for write. Pending
hints survive failure/restart and retry on account-list reads (no new poller).
Until successful RPC/backend observation, retained balances are cached snapshots,
not confirmed current on-chain holdings. Real App rendering, dependency packaging
and chain RPC are intentionally not exercised. No large-account performance claim.
