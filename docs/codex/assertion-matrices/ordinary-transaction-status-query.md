# Ordinary transaction status query — WALLET-2967 confirmed defects

Scope: ordinary TransactionService query error reporting and App SDK call log
semantics. The follow-up changes ordinary EVM missing-receipt timeout handling and adds observational send/visibility diagnostics. No migration, recovery, nonce allocation, replacement or replay. Public bill schema remains unchanged.

| Scenario | Required result / durable invariant | Regression |
| --- | --- | --- |
| Receipt RPC error, including old bill | API error; stored Pending unchanged; safe stage diagnostic | `transaction_query_tests_rpc_error_is_visible_and_does_not_expire_bill` |
| Missing local bill ID | API error, no RPC | `transaction_query_tests_missing_id_is_visible` |
| Invalid ID followed by valid expired bill | Report error after independent processing; expired ordinary EVM stays pending and queryable | `transaction_query_tests_mixed_batch_continues_and_retry_reads_expired_state` |
| Fresh missing receipt | Return Pending | `transaction_query_tests_null_receipt_preserves_fresh_pending` |
| Missing receipt past existing expiry | Ordinary EVM remains Pending; no elapsed-time chain failure | `transaction_query_tests_expiry_remains_pending_and_queryable` |
| RPC recovers | Retry re-queries, retains fresh Pending | `transaction_query_tests_recovery_retries_receipt_without_false_failure` |
| Existing terminal success / empty input | Return without RPC; empty input remains successful empty result | `transaction_query_tests_terminal_success_and_empty_batch_remain_successful` |
| Multisig persistence failure | Roll back bill confirmation | existing `pending_multisig_sync_rolls_back_when_queue_update_fails` |
| App non-200 / parser failure | Never print SDK call success; preserve error propagation or opt-in errorResponse parser | `wallet_client_handler_test.dart` |
| App successful response | Success printed only after parser finishes; existing redaction unchanged | `wallet_client_handler_test.dart` |

Queries remain sequential and each successful item commits independently; later
item errors do not roll back earlier successful updates. A mixed batch returns an
error, and a retry re-reads committed terminal results. No new concurrency or retry
mechanism is introduced. Test fixture HTTP is loopback only and SQLite is temporary.

The new timeout rule applies only to ordinary EVM transfer bills (is_multisig=0,
tx_kind=1). Existing terminal bills are not reopened. Other bill kinds/chains retain
the old policy. Late successful/reverted receipts update the real durable terminal
state (`transaction_query_tests_late_receipt_resolves_expired_pending_success_or_failure`).
Multisig/non-transfer expiry is guarded by `transaction_query_tests_multisig_and_non_transfer_expiry_keep_existing_policy`.

Broadcast diagnostics preserve the sender gate, preflight, nonce RPC, fee and amount
semantics; Core split signing is reused without persistent raw storage. Same-RPC
visibility is bounded to three read-only requests (200/500/1000ms sleeps, each <=2s),
retaining no Context/key/gate/DB. It is best-effort and cannot reject a node ack or
mutate a bill. Only opaque endpoint fingerprints, hashes, nonce/chainId, actual signed gas/fee caps, fixed
categories/numeric error codes are logged. Raw/keys/URLs/provider bodies are excluded.
`evm_submission_tests` exercises explicit null vs RPC error/malformed/hash mismatch,
pending/included visibility, bounded read-only probes and signed ack/error privacy.
Malformed acknowledgements never start a probe or appear in its logs
(`malformed_ack_is_not_queried_or_logged`); original broadcast return contract is
unchanged.
The existing 28 EVM fee/gate component cases cover final amounts/testnet fee parity.

App mode tagging is transient and excludes API/multisig bills. Expired ordinary EVM
Pending displays '超时未确认'; success/failure/other chains remain unchanged, no UI
query-error status or persistent schema change. `ordinary_transaction_timeout_test.dart`.

## Incident evidence (read-only, 2026-10-29)

Original hash `0xd47ba6639310f2ae91dd05c719f4aa5e73025cdf16a7c1c8af7119249f8ca077`
is in block96364280, hash `0xe118774762d2d5902a021b8e871f9a91edc3a1e493d312bddca808705c71d301`,
Beijing2026-10-29 13:29:59, nonce8, receipt status0x0. Nonces6/7 succeeded first in
that block; 8/9 failed. A second public RPC's original transaction call trace returns
`ERC20: transfer amount exceeds balance`. Historical USDC balance before/after block
is2.88/0.68, consistent with two successful1.1 transfers. Evidence is saved under
`target/wallet-2967-followup-*.json`, `wallet-2967-nonce-*-receipt.json`,
`wallet-2967-drpc_{before,after,trace}.json`. The archived App bill275 matches the mined nonce6 hash and was created2026-10-27.
Its maxFeePerGas=107.8gwei was below sampled base fees246.823605958gwei at
2026-10-28 10:44:42 (block96300074) and252.585531828gwei at18:14:02
(block96318042). At those sampled blocks nonce6 could not be included, and nonce8
necessarily waited behind it. Block96364280 base fee is83.459033337gwei (see raw header
for exact value). These samples do not prove every intervening block was above
the fee cap; original-node mempool contents remain unknown. No real signing or broadcasting was performed by this task.

Installed binary/device acceptance and SDK/plugin publication are not verified;
no commit/push authorized for this follow-up.

## Shared API/ordinary visibility reuse (2026-10-29)

One `domain/chain/tx_visibility` helper owns getTransactionByHash request/decoding
and the existing API200/500/1000ms attempt schedule. Wallet-specific state handling
stays at the original call sites. API matching-ack bypass, duplicate visibility
checks,401 refresh/breaker logic and SOL cached-probe behavior are unchanged.
Ordinary diagnostics retain malformed-ack rejection, matching-hash validation,
per-request2s timeout, bounded read-only lifetime and fixed/sanitized logging.

| Compatibility boundary | Evidence |
| --- | --- |
| API null/missing remains EmptyResult; non-null remains seen;401/429 classifications retained | `api_visibility_legacy_contract_characterization` first passes against the old Core path, then compares it with shared helper on identical HTTP responses |
| Ordinary explicit null is absent, malformed/error/mismatched identity is query_error | existing `visibility_distinguishes_null_error_identity_and_inclusion` now calls shared helper |
| One common schedule, three attempts and cancellable sleeps | `shared_visibility_schedule_preserves_delays_count_and_cancellation` |
| Invalid ack never queried/logged; signed ack/error diagnostics private | existing `malformed_ack_is_not_queried_or_logged`, `signed_submission_records_ack_and_error_without_raw_or_provider_body` |
| API receipt/ack/duplicate handling retained | existing broadcast/recover classification and targeted API-wallet transfer integration cases |

The explicit lookup contract preserves the legacy API null behavior; changing that
behavior is outside this refactor. Sharing protocol and schedule does not move
orders, nonce counters, signed-intent ownership, retries or chain terminal writes.
This refactor adds no App, Core, database, migration or dependency changes. Source
and validation fingerprints are frozen before independent review. No commit/push.

API compatibility also covers malformed raw JSON and wrongly typed error fields:
the shared decoder retains TransportError::Utils wrapping, so existing outward
error mapping remains531 rather than520. Local HTTP cases are compared to Core's
unchanged invoke_request path. This was corrected after independent review.

## Persistent ordinary receipt observation (2026-10-29)

- Shared API self-transfer lifecycle: startup jitter, periodic skip scheduling,
  cancellation while idle or in flight. Independent API/Core loops in one
  existing actor task; API receipt/rebroadcast behavior unchanged.
- Core Pending transfer bills are durable recovery facts; keyset pages32,
  max4 concurrent reads, per-bill5s deadline; no age/total-attempt cutoff.
- `transaction_query_tests_background_keeps_retrying_and_rediscovers_on_restart`:
  5 error rounds preserve Pending; a new cursor discovers stored bill, saves
  success, terminal bill is not queried again.
- `transaction_query_tests_background_timeouts_do_not_exhaust_and_later_revert_is_saved`:
  5 deadline failures preserve Pending, subsequent real reverted receipt saves
  Failed. Uses20ms injected deadline for deterministic local RPC stalls.
- `transaction_query_tests_background_pagination_does_not_starve_later_bills`:
  33+ rows span two pages; non-EVM, multisig and other-kind exclusions remain.
- `transaction_query_tests_background_loop_recovers_without_ui_and_cancels_inflight`:
  real scheduled rounds recover without UI; shutdown drops blocked RPC, new
  observer rediscovers Pending and saves real receipt. Existing expired fixture
  is2hours old: timeout display/age never removes it from recovery.
- No schema/dependency change, signing/broadcasting, or resetting old terminal
  failed rows. Real mobile suspension/kill and production-node behavior untested;
  process restart obtains work from persisted Core bills.
