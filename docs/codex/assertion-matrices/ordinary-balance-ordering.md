# Ordinary balance ordering / EVM asset add (2026-10-15)

Baseline 5e5bda1f; delivery is limited to SDK. Real Manager/service, temporary SQLite, scripted localhost RPC/backend, no real key/sign/broadcast.

| Contract | Regression evidence |
| --- | --- |
| Newer observation wins for same Context/asset | late_old_response, single_coin_and_bulk_refresh, backend crossing, stale_bill_balance; request registered before remote read; guard spans check and DB commit only |
| Errors/timeouts/cancellation preserve recoverability | rpc_failure, transport_timeout, partial_failure, failed_newer/cancelled refresh; weak registry cleanup units |
| Unrelated assets/Contexts and original backend scope stay independent | Context isolation, other-asset lock unit, hidden/inactive/empty backend account tests |
| Bill confirmation remains durable when balance stale | stale_bill_balance_keeps_new_balance_and_confirmation; existing transaction rollback unit |
| Ordinary/App add accepts EVM equivalent case | equivalent_contract_case, uppercase_metadata_and_concurrent_add, app_add_coin; exact-case repeated add preserves row and nonzero balance |
| Non-EVM case, chain/wallet separation preserved | non_evm_contract_case_remains_distinct, same_contract_is_independent_across_chains, add_isolated_to_selected_wallet |

RED on latest baseline: old=1 overwrote newer=2; uppercase equivalent add returned success with0 rows vs expected1. Logs: target/evidence/delivery-20261015/red-balance.log and red-case.log. These are controlled tests, not user-device incident attribution.

Implementation: App add_coin/add_regular_coin -> AssetsService::add_coin_v2 -> CoinRepo -> CoinDao EVM-only NOCASE. Bulk RPC, single-coin RPC, backend address/wallet and bill observations share Context-local RefreshOrder. SQL remains DAO-only. No public signature/database schema changes.

Limits: one shared Context only; multiple Contexts/processes sharing a DB are outside this guarantee. No startup/full signing/broadcast/App-binary verification or scale benchmark. Newer failed/cancelled requests invalidate older observations; a subsequent refresh recovers. Task suppression is test-only and Context-scoped, with drop cleanup; no key bypass added.

## Current-source verification

Fresh latest-baseline runs all exit0: balance/add19 (37.83s), existing ordinary default assets/catalog15 (1.40s), ordering lock lifecycle2 (0.02s), existing bill rollback1 (0.03s), related database assets8 (0.23s). Total45 unique tests,0failed,0ignored. Eight unrelated transfer tests from the old52 selection are deliberately excluded; one App add_coin test is added. Prior52-test result is not reused.

Exact cargo commands/results: target/evidence/delivery-20261015/results.json and corresponding green-*.log. API commands use --no-default-features --features dev,integration-tests,api-wallet-console --offline --locked; all run serially (--test-threads=1). Production/test source frozen under source-sha256-final.json (19 files, manifest SHA256 f25cfe02d59115f5664b7cb9fd2e1423d287ae2a746e3677e64b08a3c81df305 before this evidence-only update).

Independent read-only reviewer /root/independent_review: no blocking correctness findings; test SQL/receipt setup moved to flow-private support and re-reviewed, assertions preserved. cargo fmt --all -- --check and git diff --check pass. Existing SDK/dependency warnings remain; unused test RPC calls helper warning is non-functional.

DoD: [x] regressions added/updated; [x] offline local boundaries; [x] assertion matrix updated; [x] specified affected tests passed. CI API unauthenticated read currently returns404; no CI success claim. SDK-only delivery: plugin2.1.0-dev.299 remains pinned to5e5bda1f until separately updated/released.
