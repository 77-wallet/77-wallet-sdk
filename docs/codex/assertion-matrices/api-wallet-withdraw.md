# API Wallet Withdraw Assertion Matrix

Scope: `wallet-api` API wallet withdraw flow.

- Confirmed TRON token resource recovery: when MQTT writes success and
  `transaction_time` before receipt resources, NeedRecover remains eligible and
  conditionally backfills actual energy/bandwidth without changing terminal facts.

### Withdraw Platform Delegation Fact Split

- Layer: repository component and shadow predicate unit tests.
- Decision: `NeedPlatformDelegate` is persisted before the idempotent backend apply call.
- Retry: scanner retries only while the apply response is unknown. A definite `true` records the
  dependency trade number (the original trade number when the backend omits one) and waits only for
  MQTT; a definite `false` releases the gate through `FallbackAllowed` for the TRX-fee path.
- Race: a resource result received before the apply response closes the apply fact gap, while the
  resource gate remains blocked until the result ACK is persisted.
- Build invariant: TRON BuildTx requires `resource_gate_released_at`, including a fresh worker-side
  check immediately before nonce allocation.
- Entrypoints:

  ```text
  wallet-database/src/repositories/api_wallet/withdraw.rs
  withdraw_platform_delegation_fact_gap_scans_apply_without_rechecking_resources
  withdraw_platform_delegation_result_closes_apply_gap_before_apply_response

  wallet-api/src/infrastructure/api_trans/withdraw/shadow/predicate.rs
  platform_delegate_apply_advances_only_for_persisted_fact_gap
  tron_build_requires_resource_gate_release_even_when_other_facts_are_ready
  ```

### Withdraw Broadcast Resource Reservation Reconciliation

- Layer: unit plus existing Recover flow coverage.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  broadcast_resource_reservation_needs_recovery_after_grace
  ```

- DB facts: unfinished same-owner broadcasts remain part of the pending resource
  budget regardless of age.
- Recovery: after 30 seconds, the resource gate wakes Recover for those orders
  and defers its own resource decision.
- Invariant: elapsed time alone never releases a reservation; only persisted
  chain success or failure facts remove it from the pending budget.

### Withdraw In-Memory Resource Allocation

- Layer: unit plus resource-gate flow coverage.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/resource_reservation.rs
  restore_rebuilds_each_order_once
  ```

- Allocation: the chain account snapshot is allocated atomically per address
  and trade number by the in-memory reservation manager.
- Resource type: the ledger gates ENERGY only because platform delegation does
  not supply bandwidth; bandwidth shortage remains on the existing TRX fee
  fallback.
- Restart: released but unbroadcast orders are restored into the ledger from
  persisted estimated resource facts.
- Persisted allocation: the chain ENERGY snapshot subtracts the aggregate
  estimated ENERGY of other unfinished same-owner withdraws before per-order
  memory allocation.
- Deduplication: restored in-memory reservations overlap persisted pending
  orders, so effective allocation subtracts the component-wise maximum of the
  two ledgers instead of their sum.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  withdraw_resource_ready_budget_subtracts_pending_same_address_orders
  withdraw_resource_ready_uses_chain_energy_when_no_pending_orders
  ```

- Invariant: pending bandwidth remains diagnostic because platform delegation
  supplies ENERGY only; bandwidth shortage continues through the TRX fee fallback.

### Withdraw Dispatcher Watchdog Timing

- Layer: unit.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/dispatcher.rs
  queued_intent_is_not_watchdog_tracked_until_execution_starts
  ```

- Queue: an intent waiting for a dispatcher lane permit remains in duplicate
  suppression but has no watchdog start timestamp.
- Execution: watchdog timing begins only after the permit is acquired.
- Invariant: dropping the execution guard clears both tracking structures.

Rules source: `docs/codex/testing.md` and `docs/codex/testing-strategy.md`.

## Current Standard Tests

### Withdraw Build Slot Retry Recovery

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  withdraw_reclaim_stale_build_slot_clears_and_allows_reclaim
  ```

- DB facts: a stale `building_at` without `raw_tx` or `tx_hash` can be cleared
  and reclaimed for a later BuildTx attempt.
- Retry: guarded BuildTx skips release the short-lived build slot before
  returning.
- Invariant: retryable build skips must not create a durable transaction fact or
  leave an in-flight slot that prevents the next scanner attempt.

### Recover Cooldown Isolation

- Layer: unit.
- Entrypoints: `shadow_rpc_policy.rs` and Withdraw `shadow/scanner.rs`.
- Confirmation recovery uses `recover:{trade_no}`; confirmed TRON resource
  backfill uses `recover-resource:{trade_no}`.
- Invariant: both phases retain exponential cooldown and share the same RPC
  limiter/breaker, but confirmation retries cannot delay the first resource
  backfill dispatch.

### Fresh Broadcast Confirmation Admission

- Layer: unit plus existing Recover flow coverage.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/scanner.rs
  fresh_broadcast_recover_uses_fast_admission_then_old_broadcast_uses_generic_admission
  ```

- Fresh path: an unconfirmed withdrawal broadcast no more than 120 seconds ago
  starts with the existing 3-second Recover admission policy.
- Fallback: older, future-dated, and already-confirmed records retain the
  generic Recover admission policy.
- Invariant: both paths still pass through scanner fact evaluation, dispatcher
  deduplication, the chain RPC guard, and worker terminal-fact idempotency.

### Withdraw Actual Resource Display

- Layer: unit.
- Entrypoint:

  ```text
  src/api/api_wallet/withdraw.rs
  api_withdraw_order_vo_fee_display_reads_legacy_energy_only_actual_resource
  ```

- DB facts: legacy `resource_consume` may be an energy-only scalar; current
  shadow rows persist JSON with bandwidth and energy.
- API facts: list/detail serialize `transactionFee`, `bandwidthConsume`, and
  `energyConsume` for successful orders.
- DB regression:

  ```text
  wallet-database/src/dao/api_withdraw.rs
  withdraw_resource_recovery_scans_scalar_success_and_updates_conditionally
  ```

- Invariant: completed orders never present estimated bandwidth as actual.
  Startup recovery keyset-pages through every scalar row once in batches of 20,
  serially backfills complete chain resource usage, and only updates an
  unchanged scalar value.

### Withdraw Order Notification Retry

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_notification/mod.rs
  withdraw_notification_retry_on_existing_trade_no
  ```

- Backend: one `TRANS_EVENT_ACK` with `type = WD`, `ackType = TX`
  after retry.
- DB facts: withdraw row keeps `init_status = AuditPass`; retry does not
  create duplicate TX ACK.
- Invariant: failed frontend notify must not lose the persisted withdraw order.

### Withdraw TX ACK

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_notification/mod.rs
  withdraw_tx_ack_sends_once_and_persists_fact
  ```

- Backend: exactly one `TRANS_EVENT_ACK` with `type = WD`,
  `ackType = TX`.
- DB facts: `tx_ack_sent_at` is set; scanner no longer emits `SendTxAck`.
- Invariant: repeated TX ACK worker execution must not send a second ACK.

### Withdraw TX ACK Backend Failure

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_notification/mod.rs
  withdraw_tx_ack_backend_failure_keeps_fact_unset_and_retryable
  ```

- Backend: one failed `TRANS_EVENT_ACK` attempt with `type = WD`,
  `ackType = TX`.
- DB facts: `tx_ack_sent_at` stays `NULL`; scanner still emits `SendTxAck`.
- Invariant: backend ACK failure must not persist the durable ACK fact.

### Withdraw Platform Delegation Apply Fact Gap

- Layer: component.
- Entrypoint:

  ```text
  wallet-database/src/repositories/api_wallet/withdraw.rs
  withdraw_platform_delegation_fact_gap_scans_apply_without_rechecking_resources
  ```

- Backend: none.
- DB facts: `resource_block_reason = NeedPlatformDelegate`, platform dependency facts are set,
  `resource_gate_result = NULL`, and `resource_gate_released_at = NULL`.
- Invariant: a persisted platform-delegation decision scans the idempotent apply gap but cannot
  re-enter resource evaluation or BuildTx.

### Withdraw Platform Delegation Result Before Apply Response

- Layer: component.
- Entrypoint:

  ```text
  wallet-database/src/repositories/api_wallet/withdraw.rs
  withdraw_platform_delegation_result_closes_apply_gap_before_apply_response
  ```

- Backend: none.
- DB facts: the first resource result fills `resource_gate_result` monotonically while preserving
  the platform dependency and keeping `resource_gate_released_at = NULL`.
- Invariant: a resource result may close the apply gap before the HTTP response, but it cannot
  release the withdraw gate before its result ACK.

### Withdraw Resource Result ACK Payload

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_resource_gate/mod.rs
  withdraw_resource_result_ack_uses_wd_rsc_dl_type
  ```

- Backend: one `TRANS_EVENT_ACK` with `type = WD_RSC_DL`,
  `ackType = TX_RES`.
- DB facts: resource delegation row remains addressable by
  `resource_trade_no`.
- Invariant: ACK payload must not use collect resource type.

### Withdraw Resource Gate Release

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_resource_gate/mod.rs
  withdraw_resource_result_ack_releases_origin_withdraw_gate
  ```

- Backend: resource result ACK is sent for the delegation trade.
- DB facts: `resource_gate_released_at` is set;
  `resource_gate_result = ResourceDelegationSuccess`; scanner emits `BuildTx`.
- Invariant: blocked withdraw must not build before the matching resource
  delegation succeeds.

### Withdraw Failed Resource Bypass

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_resource_gate/mod.rs
  withdraw_failed_resource_bypass_reopens_withdraw_build_flow
  ```

- Backend: resource execution receipt upload is attempted for the delegation.
- DB facts: `resource_gate_released_at` is set;
  `resource_gate_result = ResourceDelegationFailedBypass`; scanner emits
  `BuildTx`.
- Invariant: failed delegation bypass must release only the matching withdraw
  gate.

### Resource ACK Without Origin Trade

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_resource_gate/mod.rs
  withdraw_resource_result_ack_without_origin_trade_no_does_not_release_gate
  ```

- Backend: resource result ACK can be sent.
- DB facts: origin withdraw keeps `resource_gate_released_at = NULL`;
  `resource_gate_result = NULL`.
- Invariant: missing `origin_trade_no` must not release any withdraw gate.

### Resource ACK With Collect Origin Type

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/withdraw_resource_gate/mod.rs
  withdraw_resource_result_ack_for_collect_origin_does_not_release_withdraw_gate
  ```

- Backend: resource result ACK can be sent.
- DB facts: origin withdraw keeps `resource_gate_released_at = NULL`;
  `resource_gate_result = NULL`.
- Invariant: collect-origin delegation must not release withdraw gate.

### Withdraw Confirm Success

- Layer: component.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/withdraw.rs
  withdraw_confirm_success_writes_transaction_time_and_chain_success
  ```

- Backend: none.
- DB facts: `transaction_time` and `chain_success_at` are set;
  `chain_failed_at` stays empty; status becomes `Success`.
- Invariant: success confirm must not write failure fact.

### Withdraw Confirm Repeat Success

- Layer: component.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/withdraw.rs
  withdraw_confirm_repeat_success_does_not_notify_again
  ```

- Backend: none.
- DB facts: existing `transaction_time` and `chain_success_at` are preserved.
- Invariant: repeat confirm must not notify again or mutate success facts.

### Withdraw Confirm Failure

- Layer: component.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/withdraw.rs
  withdraw_confirm_failure_writes_transaction_time_and_chain_failed
  ```

- Backend: none.
- DB facts: `transaction_time` and `chain_failed_at` are set;
  `chain_success_at` stays empty; status becomes `Failure`.
- Invariant: failure confirm must not write success fact.

### Withdraw Confirm Missing Trade

- Layer: component.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/withdraw.rs
  withdraw_confirm_missing_trade_no_errors
  ```

- Backend: none.
- DB facts: no row is created.
- Invariant: pool seam must surface missing rows instead of silently
  succeeding.

### Withdraw Chain TX Actions Share Running Key

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/dispatcher.rs
  chain_tx_actions_share_running_key
  ```

- Backend: none.
- Dispatcher facts: `BuildTx`, `BroadcastTx`, and `RecoverTx` for the same
  withdraw `trade_no` map to the same running key.
- Invariant: build, broadcast, and recover for one withdraw order must not run
  concurrently.

### Withdraw Expired TRON Raw Keeps TX Hash

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  withdraw_expired_tron_raw_with_tx_hash_is_not_invalidated_for_recover
  ```

- Backend: none.
- DB facts: persisted `tx_hash` is treated as possible broadcast evidence even
  when `last_broadcast_at` is still empty.
- Invariant: recover must not clear `raw_tx` / `tx_hash` for rebuild when the
  withdraw row already has a transaction hash.

### Withdraw Missing TRON Transaction Uses Raw Expiration

- Layer: unit.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/shared_mechanism.rs
  expired_tron_missing_confirmed_fails_after_settle_buffer
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  withdraw_tron_missing_confirmed_fails_after_raw_expiration_buffer
  ```

- Backend: none.
- DB facts: the persisted TRON `raw_tx` carries the protocol
  `raw_data.expiration` timestamp.
- Invariant: missing confirmed/pending is not final until the raw transaction is
  past `expiration` plus a small settle buffer; non-TRON or missing raw data must
  keep observing.
- Worker behavior: after raw expiration, withdraw keeps its existing
  manual-review transition and does not adopt collect's rebroadcast policy.

### Withdraw Broadcast RPC Timeout Preserves Chain Facts

- Layer: unit / component.
- Entrypoint:

  ```text
  src/infrastructure/chain_rpc_guard.rs
  rpc_timeout_returns_elapsed_for_slow_future
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  ```

- Backend: none.
- Chain/RPC facts: guarded broadcast RPC await time is bounded by
  `CHAIN_RPC_GUARD_RPC_TIMEOUT_MS`.
- Invariant: broadcast timeout is treated as uncertain, preserving existing
  `tx_hash` / `raw_tx` so recover can query chain facts after the current
  `ChainTx` attempt exits.

### Withdraw Post-Broadcast Confirmation Window

- Layer: unit.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  tron_post_broadcast_confirm_delays_cover_thirty_second_window
  tron_post_broadcast_reset_restarts_fresh_recover_without_touching_other_chains
  tron_post_broadcast_short_confirm_allows_trc20_resource_recovery
  src/infrastructure/api_trans/shadow_rpc_policy.rs
  successful_broadcast_resets_pre_broadcast_recover_cooldown
  ```

- Backend: none.
- Worker facts: an eligible successful TRON broadcast schedules direct
  confirmation attempts after 3, 5, 8, 15, and 30 seconds and clears any
  confirmation cooldown admitted before the broadcast fact was committed.
- Invariant: the extended post-broadcast schedule is independent from the
  bounded 3, 5, and 8 second uncertain-broadcast retry sequence; the periodic
  scanner remains the restart and delayed-chain fallback. Resetting the TRON
  confirmation key must not clear another chain's key or the independent TRON
  resource-recovery key.

### Withdraw Resource Gate Uses Chain-First Shared Lane

- Layer: unit.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/dispatcher.rs
  resource_gate_defers_shared_permit_to_chain_first_worker
  ```

- Backend: none.
- Dispatcher facts: `EvalResourceGate` keeps dispatcher running-key dedupe but
  defers shared resource admission to the worker; Build, Broadcast, and Recover
  continue using the chain lane.
- Worker facts: collect and withdraw acquire the chain RPC guard before the
  process-wide four-permit resource gate, so neither flow can hold a resource
  slot while waiting for chain admission.
- Invariant: an audit batch must not bypass the collect/withdraw resource-gate
  pool or consume the dispatcher capacity reserved for transaction execution
  and recovery.

### Withdraw Build Balance Failure

- Layer: component + worker unit.
- Entrypoint:

  ```text
  wallet-database/src/dao/api_withdraw.rs
  withdraw_build_failure_marks_error_and_blocks_can_build_scan
  withdraw_build_failure_blocks_resource_gate_scan

  wallet-api/src/infrastructure/api_trans/withdraw/shadow/worker/shadow_withdraw_worker.rs
  withdraw_build_failure_classifies_balance_errors_as_terminal

  wallet-api/src/infrastructure/api_trans/withdraw/shadow/worker/side_effect_worker.rs
  withdraw_tx_exec_receipt_payload_includes_failure_error_code
  ```

- DB facts: `tx_ack_sent_at`, `audit_passed_at`, and
  `resource_gate_released_at` are present; `raw_tx`, `transaction_time`,
  `finished_at`, and `err_code` are absent before the failure.
- Failure fact: deterministic BuildTx balance failures write `err_code=6001`,
  `failure_stage=Build`, clear `building_at`, and recompute status to
  `SendingTxFailed`.
- Backend receipt: failed withdraw execution receipts include the local
  `ERR_600x` code in `errorCode`.
- Invariant: after the failure fact is written, `scan_can_build` must not pick
  the withdraw order again; pre-Build resource-gate balance failures must also
  be excluded from `scan_need_resource_gate`; transient Build/RPC failures
  remain retryable.

### Withdraw Fee Estimate Fast Path

- Layer: scanner unit/component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/withdraw/shadow/scanner.rs
  withdraw_bnb_try_advance_dispatches_fee_estimate_before_audit

  src/domain/api_wallet/adapter/tx.rs
  fee_estimate_snapshot_parses_default_option
  fee_estimate_snapshot_parses_top_level_amount
  fee_estimate_snapshot_formats_numeric_amount
  ```

- DB facts: any regular withdraw, `fee_estimated_at IS NULL`, `raw_tx IS NULL`,
  `transaction_time IS NULL`, `finished_at IS NULL`, `err_code IS NULL`.
- Intent: point-to-point `try_advance` dispatches `EstimateFee` before TX ACK,
  resource gate, build, broadcast, or recover intents.
- Adapter facts: multi-option responses select the adapter's default fee option;
  single-option responses read the top-level estimate. Only TRON persists
  Energy/Bandwidth.
- Invariant: a newly submitted audit-pending withdraw order should get its
  display fee snapshot through the fast path instead of waiting for the next
  periodic scanner round.

## Template Contract

## Platform Delegation Chain Availability

- A successful platform resource result records
  `ResourceDelegationSuccess` but keeps `resource_gate_released_at = NULL`.
- BuildTx remains ineligible until a fresh account-resource query observes
  enough ENERGY for the withdraw.
- While delegated ENERGY is not yet visible, the gate waits without issuing a
  second platform resource request.
- A failed platform result keeps the existing fee-fallback release behavior.

`withdraw_notification/mod.rs` uses the role-trait Given-When-Then shape:

1. Keep local immutable data in a `*Fixture`.
2. Keep environment and DB pools in `WithdrawNotificationScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep wallet, withdraw order, notification-channel, and backend failure setup
   in the flow-local `WithdrawNotificationGiven` trait.
5. Keep initial submit, retry, and TX ACK worker execution in the flow-local
   `WithdrawNotificationWhen` trait.
6. Keep surfaced error, DB facts, backend TX ACK calls, scanner retry state,
   notification payload, and idempotency assertions in the flow-local
   `WithdrawNotificationThen` trait.
7. Keep seed/load/count/assert internals behind harness role containers and
   below the Given-When-Then test body.
8. Keep wallet seed, notification channel, DB pool, scanner labels, backend ACK
   counting, and payload decryption details below the scenario layer in
   `withdraw_notification/support`.

`withdraw_resource_gate/mod.rs` is the role-trait Given-When-Then template for
resource gate flows:

1. Keep the resource gate test cases in `withdraw_resource_gate/mod.rs`.
2. Keep unique trade data in `WithdrawResourceGateFixture`.
3. Keep environment and DB pools in `WithdrawResourceGateScenario`.
4. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first test roles using generic containers from `tests/harness`.
5. Keep blocked withdraw and resource delegation facts in the flow-local
   `WithdrawResourceGateGiven` trait.
6. Keep result ACK and receipt upload worker entrypoints in the flow-local
   `WithdrawResourceGateWhen` trait.
7. Keep backend payload, gate release, no-release, and scanner build
   eligibility assertions in the flow-local `WithdrawResourceGateThen` trait.
8. Keep seed/load/assert internals behind harness role containers and below
   the Given-When-Then test body.
9. Keep SQL setup below the scenario layer in
   `withdraw_resource_gate/support/db.rs`.
10. Keep backend payload wait/decrypt/assert details in
   `withdraw_resource_gate/support/assertions.rs`.

`tests/harness` remains reserved for cross-flow environment and fake
capabilities. `src/testkit` remains reserved for crate-private worker or
scanner entrypoints.

## Gaps To Close Next

- TX result ACK ordering, integration:
  `tx_res_ack_sent_at` stays `NULL` until `tx_res_received_at` exists.
- Withdraw receipt upload idempotency, integration:
  `tx_exec_receipt_uploaded_at` is written once; duplicate worker run makes
  no second upload.
- Concurrent TX ACK execution, integration:
  same `trade_no` can produce at most one backend ACK.

### Shared nonce alignment safety (2026-09-07)

Pre-broadcast alignment now preserves the monotonic allocation floor and reports
unresolved reservations rather than rewinding it. The existing withdrawal caller
keeps broadcasting its original signed transaction on alignment error; this batch
changes its diagnostic wording, not that policy. Shared allocator regressions in
transfer_nonce cover preservation, concurrency, upward progress and RPC/freeze
failures. No automatic gap fill or replacement transaction is introduced.

### Withdrawal chain-aware nonce allocation (batch 2b)

Withdrawal BuildTx's real get_nonce method now uses chain-aware atomic allocation
for ETH/BNB. Existing signed transaction and broadcast policies are unchanged.
The transfer_nonce suite uses the existing BNB fixture, fake chain observations
and real nonce repository through a testkit withdrawal worker allocation entrypoint:
- `withdrawal_nonce_syncs_chain_progress`: local last=0; observations 9 then 15
  must allocate 9 then 15, persist 15 and issue two RPC observations.
- `withdrawal_nonce_rpc_failure_recovers_without_reservation`: failed observation
  does not allocate or send; a subsequent successful observation allocates 9.
- `withdrawal_nonce_concurrent_collect_keeps_reservations_unique`: concurrent
  collect/withdraw allocation observes chain next and returns unique 9/10.
These are allocation-step integration tests, not full withdrawal signing,
broadcast, receipt or real-network acceptance. No live transactions were sent.


### API Base/Scroll native fee totals (2026-10-07, narrowed candidate)
Scope: persisted raw order paths and fee display, Base/Scroll native and standard ERC20 transfers. Core efc3ccdc unchanged.
- Base/Scroll only: checked U256 execution cap + explicit L1 estimate; exact internal quote for collect funding.
- Metadata cannot bypass L1 observation or native/token balance gates on these two chains.
- Base Core estimate is an upper-bound reserve, not an actual final-fee claim.
- RPC failure/zero additional fee cannot create broadcastable raw facts; rebroadcast uses existing raw/hash/nonce without repricing.
- Base/Scroll pre-raw fee shortage reuses existing invalidation/advancer; the completed single fee cycle still terminates shortage. Shared address lock covers fresh-read through invalidation, serialized with the sole Build raw commit; no cross-process DAO CAS claim.
- ETH/BNB/Polygon/Avalanche/Fantom/Arbitrum retain baseline balance/gas/oracle order, metadata raw behavior, display/snapshot and testnet direct transfer paths.
- Removed broad snapshot override and direct transfer rewrite. Snapshot uses the original display-response estimate path; its existing floating display conversion/DB dependency remains. Direct transfer is baseline even for Base/Scroll; this batch does not claim every API transfer entrypoint has L1 funding protection.
- Generic old metadata balance bypass and fee precision work are deferred, not fixed for unrelated chains.
- Entrypoints: adapter/eth_tx/fee_tests.rs (api_evm_fee_*); collect/shadow/worker/fee_recovery_tests.rs.
- Shared calculate_fee_setting_from_req and build_fee_setting fully match baseline. Base/Scroll direct transfer no/invalid metadata retains old amount gate; valid metadata retains old shortcut. Only the new exact funding quote uses an isolated l1_quote_fee_setting without local balance gates. Raw executes old amount gate followed by the new total fee gate.
- Isolation regression: compile-only failure is not RED; gas-isolation-red-behavior.log confirms real missing Base native direct-transfer amount gate. Final isolated-version evidence: gas-isolation-final-0.log has 15 passed; final-1..5 freshly pass trans 75, EthTx 5, withdraw shadow 66, collect_fee shadow 21, collect worker 56 (223 total). Independent full-callee review found no blocker. Earlier 12+223 is superseded. Full order orchestration, live nodes and App acceptance remain unrun.


## Manual signed EVM SelfWithdraw scope (2026-10-08)

Confirmed supported routes: eth,bnb,polygon,avalanche,fantom,arbitrum,base,scroll.
Polygon field was rejected after build and before persist/broadcast by the old eth/bnb Repo gate. Use the existing wallet-types ChainCodes(ETH_TYPE) as the shared capability source; Rust checks, bound SQL JSON and all-chain regression loops derive from it. No repeated SQL chain enumeration. Extend the same scope through persist, accepted, pending sender/observer scan, receipt, rebroadcast claim, legacy signed SelfWithdraw projection exclusion; managed Withdraw scanner rules unchanged. No schema, nonce allocation strategy, retry budget, or real business actions change.

| Evidence | Durable facts / external effects / invariant |
| --- | --- |
| self_transfer_polygon_zero_nonce_persists_before_broadcast | Polygon nonce0 persists immutable bytes/hash, nonce floor, uncertainty before node acceptance; targeted RED invalid signed ordinary EVM transfer then GREEN. |
| self_transfer_all_evm_chains_preserve_signed_lifecycle_and_nonce_zero | All8 chains: retry idempotence, conflict no overwrite, pending scan, legacy projection exclusion, bounded/cooldown identical-byte retry, accepted not confirmed, failed receipt terminal with nonce retained. |
| self_transfer_all_non_evm_chains_rejected_without_signed_facts | NonEVM/unknown/alias chains remain rejected with0 rows; unsupported receipts rejected. |
| api_wallet_transfer_nonce_all_evm_chains_persist_send_and_confirm | Real API application with existing recording adapter:8 chains persist exact raw bytes, send once, acceptance nonterminal, observer confirms receipt without second send. |
| api_wallet_transfer_nonce_polygon_persistence_failure_never_broadcasts | SQLite injected write failure leaves no signed facts or adapter sends. |
| api_wallet_transfer_nonce_new_evm_timeout_recovers_from_failed_receipt_without_resend | All6 extended EVM chains: timed out send leaves durable uncertainty, failed on-chain receipt finalizes failure and pending scan clears; sender nonce remainsreserved, no resend. |

Existing self_transfer tests retain reopen, concurrent claims, transaction rollback/receipt retry and unsigned legacy behavior. Existing transfer_nonce flow suite retains sender lock, cancellation, wrong RPC hash, observer/restart, frozen/uncertain nonce and bounded retries. Test fixtures use no real keys or RPC; fake raw bytes cannot be broadcast to a real chain. Real Polygon device transfer is not reattempted as validation.

## Same-sender EVM broadcast ordering (2026-10-15)

Scope: backend withdrawal Broadcast; reuse the existing NonceEngine transfer gate
keyed by sender + chain. Holding the gate covers fresh durable-state/chain-nonce
observation through broadcast and its fact commit. A higher signed nonce waits
for preceding chain progress; it remains eligible for the existing scanner's
CanBroadcast retry (including restart). No signed raw/hash/nonce or allocation
floor is rewritten. Different senders/chains and non-EVM flows keep their scope.

Integration entrypoint `transfer_nonce::withdrawal_broadcast_order_*` uses the real
worker, temporary SQLite and the existing recording chain adapter with strict
nonce admission. Cases:

- `defers_16_until_14_and_15_are_sent`: reproduces incident ordering 16 before
  14/15, no early send or ERR_6008; retries broadcast 14/15/16 with original bytes.
- `waits_for_inflight_predecessor`: block 14 inside the adapter; 15 cannot enter
  until 14 completes, then both durable broadcast facts exist.
- `cancelled_waiter_preserves_signed_transaction`: cancel 15 while 14 holds the
  gate; retry sends unchanged signed 15, no leaked permit or failed order.
- `nonce_query_failure_waits_without_send`: observation failure sends nothing,
  preserves signed facts/counter and resumes once observation succeeds.
- `node_nonce_lag_keeps_order_retryable`: node rejects with nonce-too-high after
  successful observation; preserve signed intent and retry identical bytes.

Every row assertion checks raw/hash/nonce, allocation floor, last_broadcast_at,
err_code, finished_at, receipt upload and broadcast uncertainty. A predecessor
that never progresses can keep successors pending; automatic gap filling,
terminal-predecessor repair and changes to historical failed orders are outside
this fix. No live transactions are used for validation.

## Withdrawal nonce continuity and legacy gaps (2026-10-15)

Supersedes the withdrawal allocation-step expectations in batch 2b above:
BuildTx observes without consuming a nonce, then atomically saves signed raw/hash,
nonce and the shared counter. The build slot precedes observation. A competing
allocator invalidates the candidate; retry never publishes stale signed bytes.
The chain's next nonce and persisted live claims remain authoritative. Observation
never lowers the counter; a replacement commits only after accounting for every
slot in the retired tail. Other flows and unaccounted reservations remain protected.

- DB `withdrawal_signed_nonce_*`: publication, eligibility, rollback, stale and
  concurrent candidates; no counter-only reservation or raw-only publication.
- DB `withdrawal_nonce_reuse_*`: deterministic errors require no text whitelist;
  active/uncertain/other-flow/on-chain facts prevent unsafe reuse, while a fully
  failed unsent tail allows a new order to reuse the chain nonce. Old raw audit stays.
- API `withdrawal_nonce_*`: chain progress and RPC failure affect observations,
  never consume a reservation before signing; collect can still reserve that nonce.
- API `withdrawal_build_failure_retries_same_nonce_then_commits_signed_facts`:
  real BuildTx with fixture keys and fake adapter; failed build keeps counter,
  successful retry commits signed facts, duplicate command does not rebuild.
- API `withdrawal_broadcast_terminal_gap_*`: failed predecessor plus signed unsent
  followers become ERR_6008 without sending; the existing receipt worker uploads
  FAIL once per order and the next candidate is reusable.
- API `withdrawal_broadcast_cancel_during_send_keeps_durable_uncertainty`: marker
  exists before adapter entry and survives cancellation; duplicate Broadcast does
  not resend or release the potentially sent signature.
- Existing `withdrawal_broadcast_order_*` remain regression coverage for normal
  ordering, in-flight predecessor, cancelled waiter, failed RPC and node lag.

No schema migration, filler transaction, failed-order revival, device SQL repair
or real network transaction. External copies of signed raw transactions are not
cryptographically revoked by local terminal status. Legacy unknown reservations
remain fenced; this change does not claim every nonce user can never create gaps.

Receipt verification runs in the dedicated `withdraw_nonce_receipt` integration
target: WorkerTestEnv requires its own process because Context is a singleton.
Run `cargo test -p wallet-api --test mod transfer_nonce::withdrawal_ -- --test-threads=1`
and `cargo test -p wallet-api --test withdraw_nonce_receipt -- --test-threads=1`
separately. The receipt uses a local mock HTTP server, with no external backend.

Post-send safety: `withdrawal_broadcast_mismatching_ack_keeps_nonce_uncertain`
and `withdrawal_broadcast_storage_failure_after_send_keeps_nonce_uncertain`
reproduce a node response followed by a hash mismatch or failed broadcast-fact
write. Broadcast/Recover errors cannot clear a durable unresolved EVM send claim
merely because retry policy is Never; retain the original raw/hash and recover.
An explicit nonce-too-high rejection retains its existing defer path. The receipt
step fixture initializes Context without background actors. Its repeated sequential
commands demonstrate persisted idempotency, not concurrent exactly-once delivery.

`withdrawal_nonce_manual_review_error_keeps_uncertainty_and_blocks_reuse` verifies
manual-review timeout reports retain raw/hash, uncertainty and Chain failure stage;
repeat writes are idempotent and the reported error never releases its nonce.

The shared broadcast flow also preserves uncertainty for EVM nonce-too-high after
an internal authentication retry: a prior send may already have succeeded before
visibility requested refresh. Initial nonce-too-high retains existing retry behavior.
The combined two-attempt auth scenario is reviewed in code, not simulated end to end.


## Receipt observation classification (2026-10-16)

Recover-stage classification is preserved through a typed worker observation:
`Confirmed`, explicit null `Missing`, and `QueryUncertain`. Core HTTP/RPC/transport/
parse failures (including HTTP500 bodies containing `result:null`) never become
`Missing`. Missing result fields are malformed; 401 refresh policy is retained.
Collect/withdraw/fee QueryUncertain performs no order-fact mutation, missing-receipt
nonce compensation, rebroadcast or failure upload. Breaker skips also stay in this
category. Legacy public optional resource recovery callers keep their "no definitive
result" contract through a compatibility wrapper. Resource operations are Tron-only
and retain their existing recovery semantics.

A withdrawal with signed bytes, an unconfirmed broadcast and >24h uncertainty is
not failed merely for reaching the manual-review threshold: warn and continue
observing. Only the local typed confirmed-chain-failure sentinel may trigger the
Recover wrapper's failure path; remote error text cannot spoof it. Fee-child explicit null also remains pending after the 5m threshold and exhausted
retry counters: neither invalidate/rebuild signed bytes nor auto-fail based only
on missing visibility. Existing collect/withdraw real
missing-receipt nonce recovery/finalized-conflict proof, non-EVM recovery rules,
and confirmation CAS guards remain separate and otherwise unchanged.

Evidence: Core `wallet-chain-interact/tests/eth_actual_fee.rs` receipt_query tests;
SDK recover_mechanism classification/spoofed-error tests; per-worker
`receipt_observation_tests` use real repo/temporary SQLite, old uncertain orders,
no-side-effect intent assertions, null retention and same-order retry/success plus
duplicate-result idempotency. A fresh collect null stays pending; a 25h withdrawal
null does not become failed. Fee-child SQLite tests cover query errors, 25h null
with zero/exhausted retry counters, and same-order confirmation after query retry.
Existing EVM and recovery tests are rerun narrowly.
No live node, signing or real broadcast. Concurrent wallet deletion work is
excluded from this task's temporary SDK validation copy and review scope.

Core receipt classification is committed at `e2e203dbfa1d5785e522240b91cc73789e4cbae5`.
SDK pins all seven Core dependencies to that revision. Adoption validation uses
the fetched fixed revision without a local Core patch; prior local-patch
regressions remain evidence for the identical implementation.

## Shared unsigned nonce frontier (2026-10-18)
Build observation uses the persisted collect/fee/withdraw ownership ledger. An
unsigned hole before a retained live signed order can be selected; a bare counter
reservation and terminal signed slots remain blocked. Candidate commit rechecks
the same policy under the SQLite writer transaction with the original chain
frontier. Raw/hash/nonce/status publication is atomic; the high-water counter is
never rewound. Filling a hole does not cancel higher signed transactions.
Evidence: `shared_nonce_recovery_*` database component cases,
`*_new_order_uses_unsigned_five_with_saved_six` integration cases, and existing
failed/cancelled/timed-out build, rollback, race and signed restart cases. All
chain interactions in regression tests use recording adapters, never live signing
or broadcasting.

2026-10-20 nonce失败复用：withdrawal_nonce_reuse_unknown_or_post_send_failure_stage_keeps_owner覆盖缺失/未知/Chain/ACK/非法阶段保留签名占用；withdrawal_nonce_reuse_known_pre_send_failure_requires_absent_send_facts覆盖Build/Broadcast正例、保留raw与发送/uncertain/执行事实反例。公共强制写入及未校验保存不可供外部调用，由compile_fail文档测试验证。
