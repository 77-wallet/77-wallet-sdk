# API Wallet Resource Assertion Matrix

Scope: `wallet-api` API wallet foreground resource and governance operations.

## API Withdrawal Wallet Delegate And Reclaim

- Tests: `api_withdraw_wallet_delegate_uses_delegate_bill_kinds` and
  `api_withdraw_wallet_undelegate_uses_reclaim_bill_kinds`.
- Batch tests: `api_withdraw_wallet_batch_delegate_uses_batch_bill_kinds`,
  `api_withdraw_wallet_batch_reclaim_uses_batch_bill_kinds`, and the shared
  request-contract tests under `request::stake::batch_tests`.
- Detail persistence tests:
  `resource_operation_records_client_batch_delegate_for_hash_detail_lookup`,
  `api_resource_operation_batch_delegate_maps_to_batch_delegate_detail`, and
  `api_resource_operation_batch_undelegate_maps_to_batch_undelegate_detail`.
- Integration tests:
  `api_batch_delegate_success_is_queryable_from_api_bill_detail`,
  `api_batch_undelegate_success_is_queryable_from_api_bill_detail`, and
  `api_batch_delegate_failed_broadcast_has_no_api_bill_detail`.
- Single-operation integration tests:
  `api_single_delegate_success_is_queryable_from_api_bill_detail` and
  `api_single_undelegate_success_is_queryable_from_api_bill_detail`.
- Entry points: `delegate_api_withdraw_wallet_resource`,
  `undelegate_api_withdraw_wallet_resource`,
  `batch_delegate_api_withdraw_wallet_resource`, and
  `batch_undelegate_api_withdraw_wallet_resource`.
- Account facts: the owner must resolve to an API withdrawal account, and the
  signing key must be loaded through `ApiAccountDomain` rather than the normal
  wallet account/key lookup.
- UI/API facts: bandwidth and energy map to their matching delegate or
  undelegate `BillKind` values for build and broadcast notifications. Batch
  responses preserve per-address broadcast status and successful transaction
  hashes.
- DB facts: each successfully broadcast batch transaction creates one client
  `api_resource_operation` row containing owner, receiver, amount, signed raw
  transaction, hash, and transaction fee. Failed broadcast items are absent
  from the successful-broadcast collection and do not create detail rows.
- Failure facts: all batch transactions are built, fee-checked, and signed
  before the first broadcast; an invalid API withdrawal account, insufficient
  total fee balance, or signing failure therefore cannot partially broadcast
  the batch.
- Invariant: API withdrawal-wallet delegation and reclaim reuse the API-wallet
  signing/broadcast path, never create normal-wallet bills, and expose every
  successful batch hash through API wallet bill detail with the correct batch
  delegate or undelegate transaction kind.

## Resource Recovery Error Classification

- Test: `recovery_error_classifier_keeps_transient_observation_errors_uncertain`
- Runtime facts: a resource transaction has already been broadcast, but its
  recovery query encounters network, timeout, rate-limit, backpressure, or RPC
  authorization errors.
- Invariant: transient observation failures never prove chain failure and cannot
  write `chain_failed_at`; later recovery must still be able to confirm success
  and trigger reclaim even when the origin collect ended in failure.

## Resource Operation Build Slot Retry Recovery

- Test:
  `wallet-api/src/infrastructure/api_trans/resource_operation/shadow.rs`
  `resource_operation_reclaim_stale_build_slot_only_without_raw_tx`
- Entry point: platform resource operation BuildTx worker.
- DB facts: a stale `building_at` with no `raw_tx` can be cleared and reclaimed
  for another BuildTx attempt.
- Retry: guarded BuildTx skips release the short-lived build slot before
  returning, matching withdraw, collect, and fee BuildTx behavior.
- Invariant: a row that already persisted `raw_tx` and `tx_hash` is not
  reclaimed by stale-slot repair, so signed transaction facts remain durable.

## Resource Delegation Retry Backoff

- Test:
  `wallet-api/src/infrastructure/api_trans/resource_delegation.rs`
  `retry_wait_secs_follows_exp_backoff_with_cap`
- Entry point: shared resource delegation retry wait calculation.
- Runtime facts: retry waits are `60s`, `120s`, `240s`, then capped at `300s`.
- Invariant: transient RPC failures must not push resource delegation fallback
  into long waits approaching one hour.

## Resource Delegation ACK Claim

- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `resource_delegation_ack_claim_blocks_duplicate_task_ack_requests`
- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `resource_delegation_ack_claim_blocks_duplicate_result_ack_requests`
- Entry point: resource delegation task/result ACK claim helpers used before
  backend `trans_event_ack` calls.
- DB facts: claiming an ACK sets `next_retry_at` into a short future window
  without marking `task_ack_sent_at` or `result_ack_sent_at`.
- Retry: a competing scanner/worker for the same resource trade no observes
  zero claimed rows and does not send the backend request; a failed owner can
  still be retried after the claim/retry wait expires.
- Invariant: collect/withdraw shadow scanners and platform resource scanners
  must not concurrently send duplicate backend ACK requests for the same
  resource delegation row.

## Platform Resource Reclaim TX_RES Gate

- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `platform_undelegation_result_ack_requires_tx_res_received_at`
- Test:
  `wallet-api/src/infrastructure/api_trans/resource_reclaim/platform_shadow.rs`
  `platform_undelegation_recover_does_not_send_result_ack_before_mqtt_result`
- Entry point: platform resource reclaim result ACK scan and worker.
- DB facts: SDK chain recovery writes `result_received_at/result_payload`, but
  leaves `tx_res_received_at` empty; MQTT final result writes
  `tx_res_received_at`.
- UI/API facts: `TX_RES` ACK is sent only after backend final-result MQTT is
  received, keeping the flow display in backend-result-before-ACK order.
- Invariant: platform resource reclaim must not ACK `TX_RES` from SDK chain
  recovery facts alone.

## Platform Resource Delegation Confirmation Gate

- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `resource_delegation_broadcast_mark_is_pending_until_chain_success`
- Test:
  `wallet-api/src/infrastructure/api_trans/resource_chain_facts.rs`
  `broadcast_only_resource_is_not_success_uploadable`
- Test:
  `wallet-api/src/infrastructure/api_trans/resource_chain_facts.rs`
  `legacy_tx_status_success_is_not_a_chain_success_fact`
- Entry point: platform resource delegation broadcast and receipt-upload scans.
- DB facts: broadcasting a resource delegation records `tx_hash` and
  `last_broadcast_at` but leaves `tx_status` empty; chain recovery must write
  `chain_success_at` / `transaction_time` before the row becomes eligible for
  backend SUCCESS execution-receipt upload.
- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `resource_delegation_build_fact_persists_hash_and_raw_before_broadcast`
- DB facts: signed `raw_tx` and deterministic `tx_hash` are persisted together
  before `last_broadcast_at`; a competing build cannot replace either value.
- Invariant: transient failure handling cannot clear a persisted pre-broadcast
  `raw_tx` / `tx_hash`; restart recovery must rebroadcast the same signed
  transaction instead of building a second transaction.
- Invariant: uncertain-timeout rebroadcast uses a dedicated atomic invalidation
  that clears `raw_tx`, `tx_hash`, and `last_broadcast_at` together before a new
  build; ordinary rebuild reset cannot invalidate a pre-broadcast signed fact.
- Invariant: a persisted signed transaction with no `last_broadcast_at` remains
  in the execute stage; only rows with a durable broadcast fact enter chain
  recovery.
- Invariant: normal platform and local undelegation orders use the same
  pre-broadcast `raw_tx` persistence and atomic recovered-success commit as
  delegation orders; only the explicit residual-energy maintenance command may
  broadcast without an order lifecycle.
- Invariant: collect and withdraw local delegation recovery atomically persist
  chain success and terminal result before either origin resource gate advances.
- Invariant: a pre-broadcast terminal resource failure that needs a local result
  payload writes failure facts and result facts in one guarded update; it cannot
  overwrite a built or broadcast transaction lifecycle.
- Invariant: collect local-delegation failure bypass releases the origin gate
  only after the resource row durably contains the complete failure terminal;
  a zero-row stale failure cannot release the gate.
- Invariant: withdraw resource-delegation execution routes errors through the
  terminal/retry classifier; a pre-broadcast permanent failure atomically
  records its failure result and wakes the origin order, while a stale failure
  after build cannot overwrite the signed transaction lifecycle.
- Invariant: platform delegation projects a permanent-failure outcome to the
  origin gate only after the resource row contains a complete atomic failure
  terminal; a stale zero-row update cannot advance the origin order.
- Invariant: collect, withdraw, platform delegation, and local/platform reclaim
  interpret ordinary recovered `Err` as a confirmed chain failure and atomically
  persist the matching broadcast hash, `chain_failed_at`, and FAIL result.
  Recovered `Ok(None)` and TRON's explicit confirmed/pending visibility error
  remain uncertain, use the bounded uncertain/rebroadcast lifecycle, and cannot
  create a terminal failure fact.
- Shared helper: resource side-effect workers use `resource_chain_facts` for
  broadcast, success, failure, and receipt-upload eligibility.
- Uncertain fact: resource recover returning no final chain result records
  `broadcast_uncertain_since_at`, increments `broadcast_uncertain_retry_count`,
  increments `broadcast_uncertain_rebroadcast_count` when uncertain timeout
  invalidates the old broadcast lifecycle for rebuild, and clears the uncertain
  lifecycle after chain success or failure.
- Invariant: `WD_RSC_DL` / `COL_RSC_DL` SUCCESS receipts must not be uploaded
  from broadcast-only hash facts.
- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `broadcast_fact_cannot_be_overwritten_by_local_failure`
- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `deterministic_broadcast_hash_reconciles_after_build_claim_reset`
- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `recovered_chain_success_and_result_are_committed_together`
- Test:
  `wallet-database/src/repositories/api_wallet/resource_delegation.rs`
  `recovered_chain_success_rejects_conflicting_hash_without_finishing`
- Invariant: local execution failure must not overwrite a persisted broadcast
  hash or any prior broadcast lifecycle; rebuild preserves
  `last_broadcast_at` as an irreversible broadcast fact. Confirmed recovery
  must restore its deterministic hash and atomically persist the chain-success
  fact and terminal SUCCESS result, even if an uncertain rebuild cleared the
  prior pending-broadcast snapshot.
- Test:
  `wallet-api/src/infrastructure/api_trans/resource_reclaim/local_shadow.rs`
  `local_undelegation_missing_relationship_is_idempotent_success`
- Invariant: `delegated Resource does not exist` proves the local undelegation
  target state is already satisfied, so it is persisted as idempotent SUCCESS
  and must not re-enter the execution scanner. An insufficient delegated
  balance remains retryable because the remaining relationship may belong to
  another active order and must not be reclaimed blindly.
- Test:
  `wallet-api/src/infrastructure/api_trans/resource_reclaim/local_shadow.rs`
  `local_undelegation_post_broadcast_confirm_delays_cover_short_window`
- Invariant: every successful local undelegation broadcast schedules Recover
  probes at 3, 8, and 15 seconds; terminal fact checks make later probes no-ops,
  and the periodic scanner remains the durable fallback.
- Test:
  `wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs`
  `local_delegation_post_broadcast_confirm_delays_cover_short_window`
- Invariant: every successful local delegation broadcast re-dispatches its
  fact-driven execution command at 3, 8, and 15 seconds. Persisted broadcast
  facts make each probe a Recover operation, and the periodic scanner remains
  the durable fallback.
- Invariant: cross-database resource-gate projection must complete before the
  resource result ACK fact is marked locally, so a projection failure remains
  retryable instead of leaving the origin order permanently blocked.

## API Withdraw Wallet Unfreeze Extraction

- Test:
  `wallet-api/src/service/api_wallet/transaction.rs`
  `api_resource_withdraw_unfreeze_operation_maps_to_withdraw_unfreeze_bill_detail_entity`
- Entry point: API wallet resource operation detail conversion.
- DB facts: client resource operation uses
  `ApiResourceOperationType::WithdrawUnfreeze`.
- UI/API facts: bill detail maps to `BillKind::WithdrawUnFreeze`.
- Invariant: API withdraw-wallet unfreeze extraction must not be represented as
  normal-wallet stake history or as vote reward withdrawal.

- Test:
  `wallet-api/src/service/api_wallet/transaction.rs`
  `api_resource_withdraw_unfreeze_operation_stays_confirming_after_broadcast_only`
- Entry point: API wallet resource operation detail conversion.
- DB facts: broadcast facts can exist without `tx_status` or `result_status`.
- UI/API facts: broadcast-only extraction detail returns confirming status.
- Invariant: client-broadcasted API resource operations do not become success
  until chain/result confirmation is recorded.
## Local Undelegation Fast Path

- Tests:
  `historical_terminal_broadcast_reconciles_once_from_chain_fact`
  and
  `scanner_reopens_historical_terminal_undelegations_for_chain_reconciliation`
- DB facts: a local delegation or undelegation has a persisted broadcast hash
  but an older build recorded terminal failure for a transient observation.
- Invariant: terminal status and legacy error classification do not prevent one
  chain reconciliation of a persisted local broadcast. Chain success corrects
  the local result and continues reclaim; a confirmed chain failure records the
  reconciliation fact and is not selected again.
- Test:
  `scanner_blocks_existing_undelegation_until_collect_is_terminal`
- Invariant: local undelegation creation, execution, historical reconciliation,
  and transaction recovery remain blocked until the origin collect has a
  persisted terminal fact.

- Tests:
  `local_delegate_recovery_rotates_checked_candidates_behind_unchecked_records`
  and
  `local_undelegation_recovery_rotates_checked_candidates_behind_unchecked_records`
- DB facts: more unresolved broadcasts exist than the per-scan limit, and the
  oldest page has already been checked on chain.
- Invariant: unchecked local delegation and undelegation broadcasts are selected
  before previously checked uncertain rows; a fixed oldest page cannot starve them.

- Test:
  `wallet-api/src/infrastructure/api_trans/resource_reclaim/local_shadow.rs`
  `local_undelegation_fast_path_enqueues_exact_task`
- Entry point: collect completion persists a local undelegation.
- Runtime facts: the exact task is immediately enqueued to the existing reclaim
  dispatcher; periodic scanning remains the fallback when its queue is unavailable.
- Invariant: scanner pagination cannot delay a newly created local reclaim task.

- Test:
  `wallet-api/src/infrastructure/api_trans/resource_reclaim/local_shadow.rs`
  `local_undelegation_reconciliation_bypasses_full_execute_page`
- Entry point: restart reconciliation creates a missing local undelegation.
- Runtime facts: the executable scan page is already full of older rows.
- Invariant: the reconciled task is emitted in the same scan round and cannot be
  starved by the paginated executable scan.

- Tests:
  `scanner_does_not_reconcile_before_origin_collect_finishes` and
  `scanner_reconciles_missing_local_undelegation_after_restart`
- Invariant: restart reconciliation may create a missing local undelegation only
  after the origin collect has a durable terminal fact. A successful delegation
  alone never authorizes reclaim while the collect can still resume.

- Test:
  `scanner_reconciles_terminal_origin_beyond_unfinished_first_page`
- Runtime facts: unfinished origin collects fill the first missing-undelegation
  page while a later page contains an eligible terminal origin.
- Invariant: cross-database terminal checks cannot make an unfinished first page
  starve later terminal origins; the scanner traverses every candidate page.

- Test: `scanner_serializes_concurrent_scan_rounds`
- Runtime facts: warm startup and periodic ticks may request a scan concurrently.
- Invariant: one scanner instance executes at most one scan round at a time;
  subsequent rounds wait for the active paginated traversal to finish.
## Local Reclaim Dispatcher Concurrency

- Test:
  `wallet-api/src/infrastructure/api_trans/resource_reclaim/local_shadow.rs`
  `local_reclaim_execute_and_recover_share_running_key`
- Invariant: Execute and Recover intents for one resource trade number share one
  running key, while distinct local undelegations may use dispatcher concurrency.
- Runtime facts: concurrency defaults to two and is bounded to `1..=16`; the
  scanner remains the durable retry path for skipped or failed work.
