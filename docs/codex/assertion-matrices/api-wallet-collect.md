# API Wallet Collect Assertion Matrix

Scope: `wallet-api` API wallet collect flow.

Rules source: `docs/codex/testing.md` and `docs/codex/testing-strategy.md`.

### Unexecuted Collect Order Expiry

- Layer: component/unit.
- Entrypoints:

  ```text
  wallet-database/src/dao/api_collect.rs
  scan_unexecuted_created_before_selects_only_expired_pre_chain_orders

  wallet-api/src/infrastructure/api_trans/collect/shadow/dispatcher.rs
  expiry_build_and_broadcast_share_mutation_key_but_recover_is_independent
  ```

- DB facts: an order older than 24 hours with an order ACK but no raw transaction,
  transaction hash, broadcast attempt, chain result, prior error, or finish fact is
  atomically frozen with `UnknownError` and the expiry reason.
- Backend: the existing `UploadTxExecReceipt` side effect reports the persisted
  failure fact; no new backend protocol is introduced.
- Concurrency: expiry shares the per-trade mutation key with Build/Broadcast and
  the database update repeats all pre-chain predicates as a compare-and-set guard.
- Invariant: age can terminate only an order that has never entered chain
  execution; built, broadcast, recoverable, failed, and finished rows are never
  expired by this policy.

### Collect Fee TRON Uncertain Rebuild

- Layer: component.
- Entrypoint:

  ```text
  wallet-database/src/dao/api_fee.rs
  invalidate_uncertain_fee_raw_tx_for_rebroadcast_preserves_confirmed_rows
  ```

- Backend: none; the test uses a temporary SQLite database.
- DB facts: an uncertain TRON fee row may have
  `broadcast_uncertain_since_at IS NOT NULL` while `last_broadcast_at IS NULL`.
- Retry: invalidation clears raw transaction, hash, and uncertain tracking so
  the scanner can emit `BuildTx` again.
- Invariant: a row with `transaction_time IS NOT NULL` is never invalidated.

## Current Standard Tests

### Collect Risk Recipient Routing

- Layer: component with the existing test context and real temporary SQLite;
  no worker actors, backend requests, signing or broadcasts.
- Entrypoint: `collect_worker/routing_tests.rs`, `collect_routing_*` tests call
  the production `resolve_collect_to_addr` used by resource estimation and build.
- Risk marker `2` selects the configured risk recipient on Polygon, Arbitrum,
  Base, Avalanche and Scroll; normal `1` and legacy/default `0` keep the normal
  recipient. Distinct recipient fixtures prevent equal addresses masking errors.
- Retry reads the latest risk strategy rather than retaining the old recipient.
  Missing requested-chain config must error rather than use another chain.
- DB/queue invariants: recipient resolution alone does not mutate the saved
  order destination, raw transaction, hash, build slot or finish fact, and emits
  no collect intent. Build's existing address-refresh path remains unchanged.
- QA mapping: TC-COL-016 / TC-COL-018; the supplied QA XMind is not modified.
- Evidence: initial real run 2 passed/2 failed with normal returned for risk;
  final results recorded under `target/risk-collect-routing/`. This is not a
  live transaction or full worker lifecycle verification.

### Collect And Fee Build Slot Retry Recovery

- Layer: component.
- Entrypoints:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  reclaim_stale_build_slot_clears_and_allows_reclaim

  src/infrastructure/api_trans/collect_fee/shadow/worker/shadow_fee_worker.rs
  fee_reclaim_stale_build_slot_clears_and_allows_reclaim
  ```

- DB facts: stale collect and fee `building_at` slots without raw transaction
  facts can be cleared and reclaimed.
- Retry: guarded BuildTx skips release the short-lived build slot before
  returning.
- Invariant: retryable build skips must not block the next scanner attempt.

### Collect Resource Gate RPC Queue Recovery

- Layer: dispatcher unit test; no DB, backend, or chain RPC.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/dispatcher.rs
  resource_gate_defers_shared_permit_to_chain_first_worker
  ```

- DB facts: RPC unavailability, timeout, and an open circuit breaker do not
  write `resource_gate_released_at` or `FallbackAllowed`.
- Admission: the dispatcher keeps the running-key marker but does not acquire a
  shared resource permit before the chain RPC guard.
- Concurrency: collect and withdraw resource gates share one small fixed
  process-local pool with four permits.
- Queueing: the worker acquires the shared resource permit only after chain RPC
  admission, preventing a 30-second chain-guard wait from occupying one of the
  four resource slots.
- Restart: after process restart, pending rows are scanned by `created_at ASC`
  and rebuild the process-local queue from oldest to newest.
- Invariant: technical RPC failure must never be converted into a business
  decision to bypass platform/local resource delegation.

### Collect Energy-Only Resource Reservation

- Layer: unit; no DB, backend, or chain RPC.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  tron_collect_resource_gate_ignores_bandwidth_shortage
  collect_resource_ready_budget_subtracts_pending_same_address_orders
  tron_resource_ready_bandwidth_fee_shortage_opens_fee_cycle_without_reopening_gate
  ```

- Admission: the collect resource gate reserves only estimated Energy because
  its delegation task cannot supply Bandwidth.
- Concurrency: pending and process-local reservations still reduce effective
  Energy before a new collect is admitted.
- Fee fallback: if BuildTx later needs TRX for Bandwidth, the released collect
  enters the service-fee cycle without clearing `ResourceReady` facts or
  releasing its Energy reservation.
- Invariant: sufficient effective Energy releases the gate regardless of a
  Bandwidth shortfall; insufficient effective Energy remains `NeedMore`, and a
  Bandwidth fee shortage cannot reopen the Energy gate.

### Collect Fresh Broadcast Recover Admission

- Layer: unit plus existing Recover flow coverage.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/advancer.rs
  collect_fresh_broadcast_recover_advancer_dispatches_with_fast_admission
  collect_fresh_broadcast_recover_wakeup_dispatches_without_scanner

  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  tron_missing_pools_recovery_action_observes_fresh_and_rebroadcasts_expired

  src/infrastructure/api_trans/shadow_rpc_policy.rs
  collect_fresh_broadcast_recover_policy_uses_fast_then_generic_admission
  ```

- Fresh path: Advancer and Scanner share the same recent-broadcast admission
  function, which starts Recover with the 3-second cooldown.
- Point-to-point retry: an uncertain TRON Recover schedules a delayed
  `try_advance` using the current shared cooldown, so the 3/8/15-second sequence
  has an active wakeup instead of depending on the periodic Scanner cadence.
- Missing pools: when an immediate TRON Recover cannot find the transaction in
  either confirmed or pending pools, a fresh order schedules the same
  point-to-point wakeup; only an order beyond the existing five-minute safety
  timeout enters invalidation and rebroadcast.
- Fallback: broadcasts older than 120 seconds retain the generic 60-second
  admission policy.
- Invariant: direct and periodic advancement cannot overwrite one another with
  conflicting cooldown classes; the DB predicate still suppresses a delayed
  wakeup after the order reaches a final state.

### Collect EVM Uncertain Transaction Preservation

- Layer: collect worker regression plus Recover flow behavior.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  broadcast_uncertain_timeout_helper_trips_after_five_minutes
  evm_uncertain_future_nonce_preserves_original_transaction_identity
  evm_uncertain_next_nonce_rebroadcasts_identical_raw_once
  evm_uncertain_consumed_nonce_is_not_rebroadcast
  ```

- Success: a visible receipt continues through the existing Recover success
  path.
- Recovery: after the five-minute uncertain timeout, a future-nonce EVM
  transaction retains its original raw transaction, hash, and nonce. When its
  nonce exactly matches the chain's pending nonce, the identical raw transaction
  may be rebroadcast once without rebuilding or changing its hash.
- Failure: a consumed or already-retried nonce is preserved for observation and
  is not broadcast again. EVM timeout never force-aligns the order nonce or
  marks the order failed; SOL and TRON policies are unchanged.
- Invariant: a database retry decision cannot erase the identity of an EVM
  transaction that may still exist in a node mempool and later be mined.

### Collect Single Service-Fee Cycle Terminal Guard

- Layer: collect worker regression.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  evm_token_collect_fee_shortage_after_fee_cycle_terminates
  evm_native_collect_fee_shortage_after_fee_cycle_terminates
  sol_token_collect_fee_shortage_after_fee_cycle_terminates
  tron_token_collect_fee_shortage_after_fee_cycle_terminates
  tron_native_collect_fee_shortage_after_fee_cycle_terminates
  ```

- Recovery: the first fee shortage still enters the existing service-fee
  cycle.
- Failure: after fee upload and fee-result ACK have both completed, a remaining
  collect fee shortage becomes terminal `BalanceInsufficient`, independent of
  chain and native/token asset type.
- Invariant: a collect cannot request a second fee payment or loop through
  rebuild-only invalidation after its single fee cycle.

- Integration: `collect_build_fee_failure_after_completed_cycle_records_terminal_failure`
  verifies no reopened cycle (return 0), persisted `BalanceInsufficient`, unchanged
  fee upload/ACK timestamps and transaction payload, and no chain confirmation.

### Collect EVM Service-Fee Estimate Alignment

- Integration: `collect_eth_service_fee_upload_uses_exact_propose_maximum`
  checks the uploaded amount is 0.0000462 ETH (23,100 gas × 2 gwei), distinct
  from both fixture estimate and display maximum amounts.
- Tests: `evm_collect_fee_paths_use_propose_max_fee_only` and
  `evm_collect_fee_paths_do_not_fallback_to_display_rounded_max_fee`.
- Build admission and service-fee upload both parse ETH/BSC estimates from the
  propose-tier `feeSetting.gasLimit * feeSetting.maxFeePerGas` integer facts.
- Regression: the displayed `maxFee.amount` may be rounded to eight decimal
  places; fee upload and BuildTx admission retain the exact signed maximum.
- Failure: malformed exact fee-setting facts fail the operation instead of
  falling back to a lower display-rounded amount.
- Invariant: the service-fee request cannot use the lower summed
  `estimate_fee.amount` value or a display-rounded maximum while BuildTx
  requires the exact propose maximum.
- Boundary: ordinary-wallet and non-EVM estimation remain unchanged.

### Collect Local Delegation Fast Path

- Layer: unit plus existing local-delegation component coverage.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/advancer.rs
  collect_local_delegation_fast_path_dispatches_execute_intent

  src/infrastructure/api_trans/resource_delegation.rs
  delegation_capacity_insufficient_cache_short_circuits_zero_snapshot
  delegation_capacity_insufficient_cache_expires
  delegation_capacity_insufficient_cache_coalesces_same_owner_resource
  ```

- DB facts: the local delegation dependency is persisted before the direct
  execution intent is emitted.
- Fast path: `ExecuteLocalResourceDelegation` enters the chain dispatcher
  immediately after persistence; periodic scanning remains the restart and
  dispatch-failure fallback.
- Capacity admission: tasks for the same owner/resource serialize the live
  capacity observation and may reuse an insufficient snapshot for two seconds,
  preventing a burst from repeating the same zero-capacity chain RPC.
- Invariant: the local delegation worker still owns capacity checks, terminal
  fact persistence, and collect-gate release; sufficient capacity is never
  admitted from cache, and an insufficient snapshot expires promptly.

### Collect Order Notification Retry

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_notification/mod.rs
  collect_notification_retry_on_existing_trade_no
  ```

- Backend: none; the flow under test is frontend notification retry after a
  persisted collect order.
- DB facts: collect row is persisted with `status = Init` after the first
  notification failure.
- Notification: retry emits one frontend event with `event = COLLECT` and the
  expected `uid`, `fromAddr`, `toAddr`, and `value`.
- Invariant: failed frontend notification must not lose the persisted collect
  order, so retrying the same `trade_no` can notify again.

### Collect SOL Token Service Fee Admission

- Layer: database.
- Entrypoint:

  ```text
  wallet-database/src/dao/api_collect.rs
  scan_confirmed_need_service_fee_upload_requires_need_service_fee_only
  ```

- DB facts: a SOL token collect row with `need_service_fee = true` and no
  resource-gate release is eligible for service-fee upload.
- Scanner: non-TRON chains do not depend on TRON resource-gate facts, while
  unreleased TRON rows remain blocked until the gate is released.
- Invariant: SOL token collect fee or rent shortage must not be stuck behind a
  TRON-only resource gate before service-fee upload.

### Collect Resource Receipt Scanner

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  collect_scanner_emits_resource_receipt_upload_for_failed_delegation
  ```

- Backend: none.
- DB facts: failed resource delegation row is ready for receipt upload scan.
- Scanner: emits `UploadResourceTxExecReceipt`.
- Invariant: failed delegation rows must remain observable by the scanner.

### Collect Resource Result ACK Release

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  collect_resource_result_ack_releases_origin_collect_gate
  ```

- Backend: resource result ACK is sent for the delegation trade.
- DB facts: origin collect writes `resource_gate_released_at` and
  `resource_gate_result = PlatformDelegateSuccess`.
- Scanner: released collect becomes eligible for `BuildTx`.
- Invariant: only a successful collect-origin delegation releases the gate.

### Collect Resource Gate Monotonic Release

- Layer: database.
- Entrypoint:

  ```text
  wallet-database/src/dao/api_collect.rs
  late_resource_block_cannot_overwrite_released_collect_gate
  ```

- DB facts: the gate is released before a stale platform-apply response tries
  to mark the collect blocked again.
- Persistence: the stale blocked write affects zero rows and preserves the
  release timestamp and `PlatformDelegateSuccess` result.
- Invariant: a released resource gate is terminal and cannot move backward to
  blocked.

### Collect Platform Resource Result ACK After Chain Success

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  collect_platform_resource_result_ack_releases_gate_after_chain_success
  ```

- Backend: resource result ACK is sent after the origin collect already has
  `transaction_time`.
- DB facts: origin collect backfills `resource_gate_released_at` and
  `resource_gate_result = PlatformDelegateSuccess`.
- Invariant: chain success must not keep a successful platform delegation gate
  unreleased.

### Collect Resource Result ACK Failure

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  collect_resource_result_ack_does_not_release_gate_on_failure
  ```

- Backend: resource result ACK can be sent for the failed delegation.
- DB facts: origin collect keeps `resource_gate_released_at = NULL` and
  `resource_gate_result = NULL`.
- Invariant: failed delegation result ACK must not release the collect gate.

### Collect Resource Gate Chain RPC Guard

- Layer: component / worker orchestration.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  process_resource_gate_inner
  ```

- Backend: none; the protected calls are chain RPC adapter calls made through
  wallet-core.
- DB facts: guarded skip / timeout does not write terminal collect facts and
  leaves the order eligible for a later scanner retry.
- Scanner: `NeedResourceGate` intent dispatch is skipped while the chain RPC
  breaker is open for the order's chain endpoint.
- Invariant: transient node failures during resource-gate evaluation must not
  be amplified across many pending collect orders, and must not be treated as a
  collect business failure.

### Collect TRON Inactive Account Resource Gate

- Layer: component / resource-gate decision.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  tron_collect_inactive_resource_snapshot_requires_account_activation
  ```

- Backend: none; the test uses an in-memory resource snapshot.
- DB facts: collect resource gate should remain unreleased and record
  `resource_block_reason = NeedAccountActivation`,
  `resource_dependency_type = AccountActivation`, and a deterministic
  activation dependency trade number when persisted by the worker path.
- Scanner: collect remains ineligible for `BuildTx` while waiting on account
  activation.
- Invariant: an empty TRON account resource response must not fall through to
  energy delegation amount calculation with `energy_price = 0`.

### Collect TRON Order-Owned Delegation Amount

- Layer: unit.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  collect_delegation_amount_uses_order_required_energy
  ```

- Backend: none; the test exercises the local amount rule directly.
- DB facts: none.
- Resource delegation: platform and local delegation requests use the current
  order's full required Energy amount.
- Invariant: same-address Energy that appears available from a previous collect
  order must not shrink the new order's delegation request to a shortfall value.

### Collect TRON Platform Delegation Shortage Failure

- Layer: unit.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  platform_delegated_collect_fee_shortage_fails_instead_of_redelegating
  ```

- Backend: none; the test exercises BuildTx error classification directly.
- DB facts: a TRON token collect already released by
  `PlatformDelegateSuccess` is treated as having consumed its single platform
  resource attempt.
- Failure: if BuildTx later reports `InsufficientFeeBalance`, the collect writes
  a terminal failure fact instead of reopening resource delegation.
- Invariant: platform resource delegation is single-attempt per collect order;
  address-level `ResourceReady` shortage may re-evaluate, but platform success
  shortage must not trigger a second Energy request.

### Collect TRON Pre-Delegation Minimum Fee Reservation

- Layer: unit.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  collect_pre_delegation_fee_ignores_energy_burn_cost
  collect_pre_delegation_fee_charges_full_bandwidth_when_not_fully_covered
  ```

- Backend: none; tests exercise the fee-admission rule directly.
- Funding: before platform/local delegation, reserve the full transaction
  Bandwidth cost when the account cannot fully cover the required Bandwidth,
  plus fixed extra fee; subtract the child address's existing TRX through the
  existing shortfall calculation.
- Fallback: the full current fee estimate remains available to the existing
  post-delegation fallback when both resource providers fail.
- Invariant: potential Energy burn cost cannot reject an order before either
  Energy delegation path is attempted, while partial Bandwidth cannot be used
  as a proportional discount against the transaction's Bandwidth fee.

### Shared Local Delegation Capacity Reservation

- Layer: unit.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/resource_delegation.rs
  resource_delegation_capacity_allows_only_one_of_ten_when_one_order_fits
  ```

- Backend: none; the test exercises the shared pre-broadcast capacity ledger.
- Resource delegation: Platform and Local delegate operations share capacity by
  owner address and resource type.
- Failure: when ten concurrent orders each require the full available frozen
  Energy balance, only one reservation succeeds and the other nine fail before
  broadcast.
- Invariant: pending delegate broadcasts must be deducted from the latest chain
  capacity snapshot; `Undelegate` operations do not reserve delegation capacity.

### Local Delegate Execution Boundary

- Layer: unit / worker orchestration.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/resource_delegation.rs
  local_delegate_execution_boundary_allows_only_active_local_delegate
  ```

- Eligibility: collect and withdraw only execute tasks whose source is `Local`,
  operation is `Delegate`, and chain/terminal failure facts are absent.
- Idempotency: queued commands for already finished local delegation tasks are
  skipped before claim, recover, build, or broadcast.
- Invariant: Platform delegation and Local undelegation tasks cannot cross into
  the collect/withdraw Local Delegate execution path.

### Pre-Broadcast Local Delegation Failure Facts

- Layer: component / resource-gate projection.
- Entrypoints:

  ```text
  wallet-database/src/repositories/api_wallet/resource_delegation.rs
  terminal_failure_and_result_are_committed_together_before_broadcast
  wallet-api/src/infrastructure/api_trans/resource_delegate/platform_shadow.rs
  platform_terminal_failure_projection_requires_complete_persisted_fact
  ```

- DB facts: a terminal validation or capacity failure before build/broadcast
  persists `result_status=Fail`, `err_code`, `err_msg`, and `finished_at`.
- Chain facts: `chain_failed_at` remains null because no chain transaction was
  broadcast; recovered post-broadcast failures still write it.
- Invariant: the failure still projects back to the collect/withdraw resource
  gate and activates the existing fee fallback.

### Shared Local Delegate Execution Core

- Layer: worker orchestration.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/resource_delegate/local_execution.rs
  execute_local_delegate_step
  ```

- Shared behavior: collect and withdraw use one implementation for eligibility,
  build claim, broadcast, uncertain rebroadcast, and chain recovery facts.
- Flow ownership: collect retains resource-gate release; withdraw retains
  scanner wakeup after rebroadcast or recovered failure.
- Invariant: centralizing chain execution must not move business projection or
  scanner ownership out of the originating flow.

### Collect TRON Account Activation Block Commit

- Layer: component / worker orchestration.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  account_activation_block_commit_is_idempotent_and_blocks_collect_build
  ```

- Backend: none; the test uses temporary SQLite databases.
- DB facts: origin collect writes `resource_block_reason = NeedAccountActivation`,
  `resource_dependency_type = AccountActivation`, and deterministic
  `resource_dependency_trade_no = act_{origin_trade_no}`; activation task stores
  payer, target, and `amount = 0.000001`.
- Scanner: blocked collect is not eligible for `BuildTx`.
- Invariant: account activation must be represented as a resource dependency
  fact, not by releasing the collect resource gate or building a raw collect tx.

### Collect TRON Account Activation Verified Recheck

- Layer: component / worker orchestration.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  account_activation_verified_requeues_resource_gate_without_release
  ```

- Backend: none; the test uses temporary SQLite databases.
- DB facts: verified activation clears only the
  `NeedAccountActivation` dependency facts; it must not write
  `resource_gate_released_at` or `ResourceReady`.
- Scanner: origin collect becomes eligible for `NeedResourceGate` again and
  remains ineligible for `BuildTx`.
- Invariant: account activation completion is not proof that the TRON collect
  has enough energy; the resource gate must re-evaluate energy and delegation
  after activation is visible.

### Collect TRON Account Activation Scanner

- Layer: component / scanner.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/scanner.rs
  scan_account_activation_emits_execute_and_verify_intents
  scan_account_activation_skips_verified_tasks
  ```

- Backend: none; the tests use temporary resource DB facts.
- DB facts: pending activation rows emit execution intents; broadcasted rows
  with `tx_hash` emit verification/recover intents; verified rows emit none.
- Scanner: emits `ExecuteAccountActivation` only from activation facts.
- Invariant: scanner must not execute activation itself and must not redispatch
  already verified activation tasks.

### Collect TRON Token Resource Fee Gate

### Collect TRON Amount Shortage Before Resource Gate

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  tron_collect_amount_shortage_fails_before_resource_delegation
  ```

- Backend: fake TRON adapter returns zero balance and no real network is used.
- DB facts: collect row is marked `InsufficientBalance` with
  `BalanceInsufficient`, resource gate remains unreleased, and no resource
  delegation row exists for the origin order.
- Resource delegation: TRON collect orders that have not released the resource
  gate, built a raw transaction, confirmed, finished, or failed must check the
  collect amount before resource delegation.
- Invariant: collect amount shortage must become a collect failure without
  creating platform or local resource delegation work.

- Layer: unit / collect worker predicate.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  tron_token_collect_resource_gate_bypasses_native_fee_balance_only_after_release
  ```

- Backend: none.
- DB facts: a TRON token collect with `resource_gate_released_at` present and an
  SDK resource delegation success result (`ResourceDelegationSuccess`) is
  allowed to build even if the conservative native burn-fee estimate is higher
  than the current TRX balance.
- Invariant: the bypass is TRON-token-only and requires the resource gate
  release fact plus an SDK resource delegation success result; address-level
  `ResourceReady`, platform/local delegation success, TRON native collect,
  unreleased TRON token collect, fallback / failed-bypass resource results, and
  non-TRON token collect keep their existing native fee balance gates.

### Collect TRON Address Resource Reservation

- Layer: unit / collect worker resource gate.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  collect_resource_ready_budget_subtracts_pending_same_address_orders
  ```

- Backend: none.
- DB facts: pending `ResourceReady` collect rows persist their estimated
  resource budget in `resource_consume` until BuildTx/broadcast/recover writes
  the actual resource fact.
- Resource gate: a later TRON collect for the same `from_addr` subtracts pending
  address-level `ResourceReady` budgets before deciding whether current address
  Energy is sufficient.
- Invariant: temporary Energy observed on an address must not be counted as
  independently available to multiple unfinished collect orders.

### Collect TRON Stale ResourceReady Re-Evaluation

- Layer: unit / collect repository.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  stale_resource_ready_invalidation_reopens_resource_gate
  ```

- Backend: none.
- DB facts: an unfinished TRON token collect whose address-level `ResourceReady`
  is later contradicted by `InsufficientFeeBalance` can clear the resource gate
  release fact, pending estimate, and build slot.
- Invariant: stale address-level resource readiness must move the order back to
  `scan_need_resource_gate` instead of staying stuck in BuildTx retries.

### Collect TRON Account Activation Repository Guards

- Layer: component / repository.
- Entrypoint:

  ```text
  wallet-database/src/repositories/api_wallet/tron_account_activation.rs
  tron_account_activation_build_claim_matches_resource_delegation_guard
  tron_account_activation_retry_wait_blocks_execute_and_verify_scans
  ```

- Backend: none; the tests use temporary SQLite databases.
- DB facts: broadcast success requires a prior build claim; stale build claims
  become executable again after the same five-minute guard used by resource
  delegation; future `next_retry_at` blocks both execution and verification
  scans; `recover_status` records whether the next retry resumes build or
  recover.
- Invariant: activation task orchestration must follow resource delegation
  claim/retry semantics and must not invent a looser execution path.

### Collect Gate Ignores Withdraw-Origin ACK

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  withdraw_origin_resource_result_ack_does_not_release_collect_gate
  ```

- Backend: resource result ACK can be sent.
- DB facts: collect gate remains unreleased.
- Invariant: withdraw-origin delegation must not release a collect gate.

### Collect Failed Resource Bypass

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  collect_failed_resource_bypass_reopens_collect_build_flow
  ```

- Backend: resource execution receipt upload is attempted for the delegation.
- DB facts: collect remains blocked on platform delegation dependency.
- Scanner: collect is still not eligible for `BuildTx`.
- Invariant: failed platform delegation must not reopen collect build before
  local fallback facts exist.

### Collect Resource Receipt Without Origin

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_resource_gate/mod.rs
  collect_resource_tx_exec_receipt_failure_without_origin_trade_no_does_not_release_gate
  ```

- Backend: resource execution receipt upload can run.
- DB facts: collect gate remains unreleased.
- Invariant: missing `origin_trade_no` must not release any collect gate.

### Collect Scanner Does Not Own Local Reclaim

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_local_reclaim/mod.rs
  collect_shadow_scanner_no_longer_owns_local_undelegation_intents
  ```

- Backend: none.
- Scanner: collect scanner does not emit `ExecuteLocalUndelegation` or
  `RecoverLocalUndelegation`.
- DB facts: local undelegation task is present in the resource task table.
- Invariant: collect shadow scanner must not own local undelegation execution or
  recovery after those intents move to the local reclaim scanner.

### Collect Result ACK Creates Local Reclaim Before Finish

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/side_effect_worker.rs
  process_result_ack
  ```

- Backend: TX_RES ACK succeeds.
- DB facts: successful local delegation creates `rsc_local_undelegate_*` after
  `result_ack_sent_at` is persisted and before `finished_at` is persisted.
- Invariant: collect finalization must not happen before SDK local resource
  reclaim intent is recorded.

### Local Reclaim Scanner Owns Local Undelegation

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_local_reclaim/mod.rs
  local_reclaim_shadow_scanner_owns_local_undelegation_intents
  ```

- Backend: none.
- Scanner: local reclaim scanner emits both `ExecuteLocalUndelegation` and
  `RecoverLocalUndelegation`.
- DB facts: one local undelegation task is build-ready, and one has broadcast
  success facts for recovery.
- Invariant: local reclaim scanner must be the owner for local undelegation
  execution and recovery intents.

### Collect Fee Cycle Skips Stale Rows

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee_cycle/mod.rs
  collect_scanner_skips_stale_fee_cycle_rows
  ```

- Backend: none.
- Scanner: emits no collect intent.
- DB facts: `need_service_fee = true`, `service_fee_uploaded_at` remains set,
  and `raw_tx` / `tx_hash` remain empty.
- Invariant: stale fee-cycle residue must not re-enter build or fee-result ACK
  scanning.

### Collect Fee Cycle Uploads Service Fee

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee_cycle/mod.rs
  collect_scanner_emits_upload_service_fee_when_need_service_fee_is_true
  ```

- Backend: none.
- Scanner: emits `UploadServiceFee` and does not emit `BuildTx`.
- DB facts: `need_service_fee = true`; service-fee order and upload timestamps
  remain empty until the upload step runs.
- Invariant: fee upload gating must run before collect build when service fee is
  still required.

### Collect Fee Cycle Reopen Builds Without Upload

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee_cycle/mod.rs
  collect_scanner_builds_after_fee_cycle_reopen_without_service_fee_upload
  ```

- Backend: none.
- Scanner: emits `BuildTx` and does not emit `SendTxFeeResAck`.
- DB facts: `need_service_fee = false`, `service_fee_uploaded_at = NULL`,
  `tx_fee_res_ack_sent_at = NULL`, and execution facts remain empty.
- Invariant: a reopened fee cycle without a real upload in the current cycle
  must not be blocked by historical ACK residue.

### Collect Fee Cycle ACKs Before Build

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee_cycle/mod.rs
  collect_scanner_emits_tx_fee_res_ack_before_build_after_fee_result
  ```

- Backend: none.
- Scanner: emits `SendTxFeeResAck` and does not emit `BuildTx`.
- DB facts: `need_service_fee = false`, `tx_fee_res_ack_sent_at = NULL`, and
  `raw_tx` remains empty.
- Invariant: fee-result ACK must be sent before build is allowed after a
  completed fee upload.

### Collect Pre-Broadcast Backend Failure Finish

- Layer: component.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/confirm_tx_tests.rs
  collect_pre_broadcast_fee_fail_should_finish_without_tx_exec_receipt
  ```

- Backend: none.
- DB facts: `transaction_time = NULL`, `status = Failure`, fee-insufficient
  error facts are retained, `finished_at IS NOT NULL`, and
  `tx_exec_receipt_uploaded_at = NULL`.
- Invariant: when backend has already confirmed a collect failure before any
  broadcast evidence exists, SDK must finish locally and must not upload a
  failed TxExecReceipt back to backend.

### Collect Fee Check

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee/fee_check.rs
  collect_sol_native_fee_check_*
  collect_eth_native_fee_check_*
  collect_build_fee_estimation_shortage_reopens_fee_cycle
  ```

- Chain adapter: Solana and Ethereum transaction adapters are faked through the
  test adapter override.
- DB facts: collect rows remain in `Init` during pure fee check; fee shortage
  reopens the service-fee cycle without raw transaction facts.
- Invariant: fee-check failures must report the correct reason or reopen the
  fee cycle without mutating completed execution facts.

### Collect Service Fee Upload

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee/service_fee_upload.rs
  collect_service_fee_upload_*
  collect_eth_service_fee_upload_uses_estimated_fee_without_multiplier
  ```

- Backend: fake backend recorder captures service-fee upload requests.
- DB facts: collect row is in service-fee wait state before upload.
- Payload: service-fee upload uses the withdrawal-to-subaccount direction,
  correct token metadata, and correct amount for Solana base shortfall, Solana
  recipient ATA rent, or Ethereum estimated fee.
- Invariant: service-fee upload must bypass the local balance gate and must not
  multiply the already estimated fee amount.

### Collect Build Fee Reopen

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_fee/build_fee.rs
  collect_build_fee_failure_*
  ```

- Chain adapter: fake Solana fee adapter returns insufficient balance.
- DB facts: first insufficient-balance build attempt marks
  `need_service_fee = true`; completed fee-cycle facts keep
  `service_fee_uploaded_at` and `tx_fee_res_ack_sent_at`.
- Invariant: failed build-fee checks reopen only the needed part of the fee
  cycle and must not erase completed service-fee facts.

### Collect Receipt Payload Uses Persisted Address

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_receipt/mod.rs
  collect_tx_exec_receipt_uses_persisted_to_addr
  ```

- Backend: none.
- DB facts: none; the test uses an in-memory `ApiCollectEntity`.
- Payload: `to`, `hash`, `tradeNo`, and `status` come from the persisted
  collect execution facts.
- Invariant: receipt payload must not rebuild `to` from request defaults after
  execution facts have been persisted.

### Collect Receipt Rebuild Uses Rebuilt Address

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_receipt/mod.rs
  collect_rebuild_then_receipt_upload_uses_rebuilt_to_addr
  ```

- Backend: none.
- DB facts: stale `raw_tx` and `tx_hash` are invalidated; rebuilt `to_addr` and
  `tx_hash` are loaded for the receipt payload.
- Payload: `to`, `hash`, `tradeNo`, and `status` reflect rebuilt execution
  facts.
- Invariant: receipt upload must use rebuilt execution facts, not stale build
  facts.

### Collect Receipt Worker Upload

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_receipt/mod.rs
  collect_side_effect_worker_marks_tx_exec_receipt_uploaded_after_rebuild
  ```

- Backend: mock backend receives execute-complete upload through the worker.
- DB facts: `tx_exec_receipt_uploaded_at` is set after successful backend
  upload.
- Payload: `tradeNo`, `to`, `hash`, and `status = SUCCESS` reflect rebuilt
  execution facts.
- Invariant: worker upload must write the durable upload fact only after the
  mock backend path succeeds.

### Collect Receipt Direct Backend Upload

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_receipt/mod.rs
  collect_backend_api_direct_upload_hits_mock_server
  ```

- Backend: mock backend captures `awallet/aw/trans/executeComplete`.
- DB facts: none; direct backend upload does not mutate the collect row.
- Payload: `tradeNo`, `to`, `hash`, and `status = SUCCESS` match the request
  entity.
- Invariant: direct backend helper must use the mock backend configured by the
  integration harness.

### Collect Receipt Scanner Dispatcher

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_receipt/mod.rs
  collect_scanner_dispatcher_uploads_rebuilt_tx_exec_receipt
  ```

- Backend: mock backend receives execute-complete upload through scanner
  dispatch.
- DB facts: scanner returns the selected `trade_no` and sets
  `tx_exec_receipt_uploaded_at`.
- Payload: `tradeNo`, `to`, `hash`, and `status = SUCCESS` reflect scanner-ready
  execution facts.
- Invariant: scanner-dispatcher must upload the rebuilt receipt once and
  persist the durable upload fact.

### Collect Blockhash Rebuild Clears Stale Facts

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_recovery/mod.rs
  collect_blockhash_rebuild_clears_stale_build_facts_and_persists_new_to_addr
  ```

- Backend: none.
- DB facts: stale `raw_tx` and `tx_hash` are cleared before rebuild; persisted
  `to_addr` is updated only by the next build step.
- Invariant: rebuilding after an expired blockhash must not keep stale execution
  facts or silently invent a replacement execution address.

### Collect Recover Queries Chain Before Rebuild

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_recovery/mod.rs
  collect_recover_queries_chain_before_any_expired_raw_rebuild_invalidation
  ```

- Backend: none.
- Chain: fake TRON adapter is queried once before any rebuild invalidation.
- DB facts: `transaction_time` is persisted, `last_broadcast_at` remains, and
  `raw_tx` is not cleared before the visible chain confirmation is handled.
- Invariant: recovery must prefer durable chain evidence over rebuilding an
  expired raw transaction too early.

### Fresh Collect Broadcast Fast Confirmation

- Layer: unit / scanner policy.
- Entrypoint:

  ```text
  wallet-api/src/infrastructure/api_trans/shadow_rpc_policy.rs
  fresh_recover_uses_fast_sequence_then_returns_to_default_backoff
  ```

- DB facts: a durable `last_broadcast_at` within the last two minutes selects
  the fresh-broadcast confirmation policy.
- Retry: fresh confirmation uses `3s -> 8s -> 15s -> 30s`; older transactions
  return to the existing `60s -> 120s -> 240s -> 300s` recovery backoff.
- Invariant: every confirmation query still passes through the chain RPC guard
  and circuit breaker.

### Fresh Collect Fee Broadcast Fast Confirmation

- Layer: unit / worker and scanner policy.
- Entrypoints:

  ```text
  wallet-api/src/infrastructure/api_trans/collect_fee/shadow/worker/shadow_fee_worker.rs
  tron_fee_post_broadcast_schedule_resets_only_tron_confirmation_cooldown

  wallet-api/src/infrastructure/api_trans/collect_fee/shadow/scanner.rs
  fee_recover_dispatch_uses_fresh_broadcast_window
  ```

- Broadcast: a successful TRON fee broadcast clears any pre-broadcast Recover
  cooldown and schedules direct confirmation wakeups at `3/5/8/15/30` seconds.
- Scanner: both safety scans and point-to-point advancement use the durable
  `last_broadcast_at` timestamp, so a fresh broadcast starts with the 3-second
  admission window instead of inheriting the generic 60-second cooldown.
- Fallback: broadcasts older than 120 seconds retain the generic Recover
  admission policy, and non-TRON broadcasts do not schedule direct wakeups.
- Invariant: terminal fee rows are never scheduled for post-broadcast recovery,
  and every confirmation query still passes through the chain RPC guard.

### Idempotent Collect And Fee Confirmation Race

- Layer: unit / domain orchestration.
- Entrypoints:

  ```text
  wallet-api/src/domain/api_wallet/trans/collect.rs
  collect_confirmation_status_accepts_concurrent_result_and_terminal_states
  wallet-api/src/domain/api_wallet/trans/fee.rs
  fee_confirmation_status_accepts_concurrent_result_and_terminal_states
  ```

- DB facts: when a conditional status update affects zero rows, confirmation
  reloads the persisted transaction before acknowledging the message.
- Idempotency: the expected result state and its corresponding confirmed-report
  terminal state are accepted as advancement completed by a concurrent path;
  persisted `transaction_time` also accepts legal result-before-receipt ordering
  while the derived status remains `SendingTx`.
- Failure: every other persisted status returns an error and prevents the
  confirmation message from being silently acknowledged.
- Invariant: a zero-row update alone is never treated as success.

### Collect Recover Backfills Missing Hash

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_recovery/mod.rs
  collect_recover_backfills_missing_tx_hash_before_receipt_upload
  ```

- Backend: none.
- Chain: fake TRON adapter confirms the recovered transaction.
- DB facts: recovered `tx_hash` is restored, `transaction_time` is persisted,
  and the row becomes eligible for execution-receipt upload.
- Invariant: receipt upload readiness must not depend on a transient missing
  local hash when the chain result can provide it.

### Collect Scanner Recovers Broadcast Visible Pending

- Layer: component.
- Entrypoint:

  ```text
  tests/integration/api_wallet/collect_recovery/mod.rs
  collect_scanner_recovers_broadcast_visible_pending_result
  ```

- Backend: none.
- Scanner: emits `RecoverTx`, and does not emit `BuildTx` or
  `UploadServiceFee`.
- DB facts: pending row keeps `tx_hash`; `transaction_time` remains empty until
  recovery confirms the broadcast.
- Invariant: scanner routing must recover broadcast-visible pending rows instead
  of re-entering build or fee upload.

### Collect Chain TX Actions Share Running Key

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/dispatcher.rs
  chain_tx_actions_share_running_key
  ```

- Backend: none.
- Dispatcher facts: `BuildTx`, `BroadcastTx`, and `RecoverTx` for the same
  collect `trade_no` map to the same running key.
- Invariant: build, broadcast, and recover for one collect order must not run
  concurrently.

### Collect Expired TRON Raw Keeps TX Hash

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  collect_expired_tron_raw_with_tx_hash_is_not_invalidated_for_recover
  ```

- Backend: none.
- DB facts: persisted `tx_hash` is treated as possible broadcast evidence even
  when `last_broadcast_at` is still empty.
- Invariant: recover must not clear `raw_tx` / `tx_hash` for rebuild when the
  collect row already has a transaction hash.

### Collect Fee Chain TX Actions Share Running Key

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect_fee/shadow/dispatcher.rs
  chain_tx_actions_share_running_key
  ```

- Backend: none.
- Dispatcher facts: `BuildTx`, `BroadcastTx`, and `RecoverTx` for the same fee
  `trade_no` map to the same running key.
- Invariant: build, broadcast, and recover for one fee order must not run
  concurrently.

### Collect Fee Expired TRON Raw Keeps TX Hash

- Layer: component.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect_fee/shadow/worker/shadow_fee_worker.rs
  collect_fee_expired_tron_raw_with_tx_hash_is_not_invalidated_for_recover
  ```

- Backend: none.
- DB facts: persisted `tx_hash` is treated as possible broadcast evidence even
  when `last_broadcast_at` is still empty.
- Invariant: recover must not clear `raw_tx` / `tx_hash` for rebuild when the
  fee row already has a transaction hash.

### Collect Missing TRON Transaction Uses Uncertain Timestamp

- Layer: unit.
- Entrypoint:

  ```text
  src/domain/api_wallet/trans/shared_mechanism.rs
  tron_missing_confirmed_and_pending_retry_waits_for_timeout
  ```

- Backend: none.
- DB facts: historical collect and collect-fee rows may have no
  `last_broadcast_at` while retaining `broadcast_uncertain_since_at`.
- Invariant: Recover uses `last_broadcast_at` when present, otherwise falls back
  to `broadcast_uncertain_since_at`; with neither timestamp it must keep waiting.
- Worker behavior: after timeout, collect and collect-fee retain their existing
  rebroadcast transitions.

### Collect Resource Gate Uses Shared Single Lane

- Layer: unit.
- Entrypoints:

  ```text
  src/infrastructure/api_trans/shadow_rpc_policy.rs
  shared_resource_gate_semaphore_allows_only_one_cross_flow_permit

  src/infrastructure/api_trans/collect/shadow/dispatcher.rs
  resource_gate_uses_shared_lane_while_transaction_actions_use_chain_lane
  ```

- Backend: none.
- Dispatcher facts: Collect `EvalResourceGate` uses the same process-wide
  one-permit lane as Withdraw resource evaluation; Build, Broadcast, and Recover
  remain on the Collect chain lane.
- Invariant: Collect and Withdraw resource evaluations must not overlap or
  consume transaction execution and recovery dispatcher capacity.

### Chain RPC Guard Acquire Timeout

- Layer: unit.
- Entrypoint:

  ```text
  src/infrastructure/chain_rpc_guard.rs
  guarded_permit_acquire_times_out_without_permit
  guarded_permit_acquire_returns_permit_when_available
  ```

- Backend: none.
- Chain/RPC facts: guarded endpoint permit acquisition is bounded by
  `CHAIN_RPC_GUARD_ACQUIRE_TIMEOUT_MS`; a saturated semaphore returns a timeout
  result instead of awaiting forever.
- Invariant: collect-fee broadcast can end the current attempt and release its
  dispatcher running key when the shared RPC guard is saturated, without writing
  a terminal order failure.

### Chain RPC Broadcast Timeout

- Layer: unit / component.
- Entrypoint:

  ```text
  src/infrastructure/chain_rpc_guard.rs
  rpc_timeout_returns_elapsed_for_slow_future
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  src/infrastructure/api_trans/collect_fee/shadow/worker/shadow_fee_worker.rs
  ```

- Backend: none.
- Chain/RPC facts: guarded broadcast RPC await time is bounded by
  `CHAIN_RPC_GUARD_RPC_TIMEOUT_MS`.
- Invariant: broadcast timeout is treated as uncertain, preserving existing
  `tx_hash` / `raw_tx` so recover can query chain facts after the current
  `ChainTx` attempt exits.

### Chain RPC 503 Circuit Isolation

- Layer: unit / component.
- Entrypoint:

  ```text
  src/infrastructure/chain_rpc_guard.rs
  rpc_circuit_breaker_opens_quickly_for_503
  rpc_circuit_breaker_keeps_normal_failure_threshold
  rpc_circuit_breaker_half_open_allows_one_probe
  ```

- Backend: none.
- Chain/RPC facts: 503 failures use a stricter endpoint-local breaker policy;
  the runtime network, chain code, and RPC host form the breaker identity.
- Invariant: testnet and mainnet failures do not share breaker state, and a
  cooled-down endpoint admits only one half-open probe.

### Collect Build Service Unavailable Retry

- Layer: unit.
- Entrypoint:

  ```text
  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  collect_build_service_unavailable_remains_retryable_regardless_of_updated_at
  ```

- Chain/RPC facts: BuildTx has no `raw_tx` or `tx_hash`, and the node response
  is classified as service unavailable.
- DB facts: the build slot is released by the worker; no terminal transaction
  error or local retry-window timestamp is written.
- Invariant: mutable order `updated_at` never controls BuildTx 503 finality;
  retry pacing belongs to the shared endpoint circuit breaker.

## Template Contract

`collect_notification/mod.rs` uses the role-trait Given-When-Then shape:

1. Keep unique collect input data in `CollectOrderFixture`.
2. Keep environment and DB pool in `CollectNotificationScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep wallet and notification-channel setup in the flow-local
   `CollectNotificationGiven` trait.
5. Keep initial submit and retry entrypoints in the flow-local
   `CollectNotificationWhen` trait.
6. Keep surfaced error, DB retry fact, and notification payload assertions in
   the flow-local `CollectNotificationThen` trait.
7. Keep seed/load/assert internals behind harness role containers and below
   the Given-When-Then test body.
8. Keep wallet seed, notification channel, DB pool, and payload serialization
   details below the scenario layer in `collect_notification/support`.

`collect_receipt/mod.rs` uses a mixed-layer receipt contract:

1. Keep payload-only and local SQLite receipt tests as component-style
   Arrange-Act-Assert tests.
2. Keep unique integration receipt input data in `CollectReceiptFixture`.
3. Keep environment and DB pools in `CollectReceiptScenario`.
4. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
5. Keep mock-backend verification, rebuilt execution facts, and scanner-ready
   facts in the flow-local `CollectReceiptGiven` trait.
6. Keep worker, direct backend, and scanner receipt upload entrypoints in the
   flow-local `CollectReceiptWhen` trait.
7. Keep durable DB upload facts, receipt payload facts, backend
   execute-complete capture, and selected scanner trade assertions in the
   flow-local `CollectReceiptThen` trait.
8. Keep seed/load/assert internals behind harness role containers and below
   the Given-When-Then test body.
9. Keep low-level details such as SQL updates, payload serialization, and
   backend body decryption below the scenario layer in
   `collect_receipt/support`.

`collect_fee/service_fee_upload/mod.rs` uses the role-trait Given-When-Then
shape:

1. Keep service-fee upload cases read-first in `service_fee_upload/mod.rs`.
2. Keep worker environment and recorder reset in
   `CollectServiceFeeUploadScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep chain fee adapter installation and waiting service-fee rows in the
   flow-local `CollectServiceFeeUploadGiven` trait.
5. Keep upload worker execution in the flow-local
   `CollectServiceFeeUploadWhen` trait.
6. Keep payload route, trade number, and amount assertions in the flow-local
   `CollectServiceFeeUploadThen` trait.
7. Keep adapter details, worker helper calls, recorder payload loading, and
   floating-point tolerance assertions below the Given-When-Then test body.

`collect_fee/build_fee/mod.rs` uses the role-trait Given-When-Then shape:

1. Keep build-fee reopen cases read-first in `build_fee/mod.rs`.
2. Keep worker environment and collect DB pool in `CollectBuildFeeScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep fake low-balance adapter setup, collect row seeding, completed
   fee-cycle facts, and collect reloads in the flow-local
   `CollectBuildFeeGiven` trait.
5. Keep fee-check and build-attempt invalidation entrypoints in the flow-local
   `CollectBuildFeeWhen` trait.
6. Keep failed-check, affected-row, reopened-cycle, and preserved-cycle
   assertions in the flow-local `CollectBuildFeeThen` trait.
7. Keep SQL setup, worker construction, DB loads, and seed/load/assert internals
   below the Given-When-Then test body.

`collect_fee/fee_check/mod.rs` uses the role-trait Given-When-Then shape:

1. Keep fee-check cases read-first in `fee_check/mod.rs`.
2. Keep worker environment and collect DB pool in `CollectFeeCheckScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep fake Solana/Ethereum adapter setup and collect row seeding in the
   flow-local `CollectFeeCheckGiven` trait.
5. Keep Solana/Ethereum fee-check entrypoints and build-attempt invalidation in
   the flow-local `CollectFeeCheckWhen` trait.
6. Keep fee-check result, error message, collect status, affected row, and
   reopened-cycle assertions in the flow-local `CollectFeeCheckThen` trait.
7. Keep worker construction, DB loads, and seed/load/assert internals below the
   Given-When-Then test body.

`collect_resource_gate/mod.rs` uses the role-trait Given-When-Then shape for
resource gate flows:

1. Keep resource gate test cases in `collect_resource_gate/mod.rs`.
2. Keep unique trade data in `CollectResourceGateFixture`.
3. Keep local scanner-only DB setup in `LocalCollectResourceDb`.
4. Keep environment and DB pools in `CollectResourceGateScenario`.
5. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first test roles using generic containers from `tests/harness`.
6. Keep blocked collect and resource delegation facts in the flow-local
   `CollectResourceGateGiven` trait.
7. Keep resource result ACK and receipt upload worker entrypoints in the
   flow-local `CollectResourceGateWhen` trait.
8. Keep gate release, no-release, platform dependency, and build eligibility
   assertions in the flow-local `CollectResourceGateThen` trait.
9. Keep seed/load/assert internals behind harness role containers and below
   the Given-When-Then test body.
10. Keep SQL setup and scanner/build plumbing below the scenario layer in
    `collect_resource_gate/support/db.rs`.

`collect_local_reclaim/mod.rs` uses a local scanner ownership contract:

1. Keep scanner ownership cases read-first in `collect_local_reclaim/mod.rs`.
2. Keep local SQLite setup and local undelegation seed data in
   `LocalReclaimScannerDb`.
3. Use `given_*` methods for local undelegation setup and broadcast-success
   recovery setup.
4. Use `when_*` methods for collect scanner and local reclaim scanner rounds.
5. Use `then_*` methods for scanner label ownership assertions.
6. Keep repository setup and scanner helper calls below the scenario layer in
   `collect_local_reclaim/support.rs`.

`collect_fee_cycle/mod.rs` uses a local scanner routing contract:

1. Keep fee-cycle scanner cases read-first in `collect_fee_cycle/mod.rs`.
2. Keep unique collect trade data in `CollectFeeCycleFixture`.
3. Keep local SQLite setup, collect seed data, fee-cycle SQL facts, scanner
   calls, and DB reloads in `LocalCollectFeeCycleDb`.
4. Use `given_*` methods for stale, waiting, reopened, and completed fee-cycle
   facts.
5. Use `when_*` methods for collect scanner rounds.
6. Use `then_*` methods for scanner label routing and persisted DB fact
   assertions.
7. Keep SQL setup and scanner helper calls below the scenario layer in
   `collect_fee_cycle/support.rs`.

`collect_recovery/mod.rs` uses a mixed-layer recovery contract:

1. Keep recovery test cases read-first in `collect_recovery/mod.rs`.
2. Keep unique trade and hash data in `CollectRecoveryFixture`.
3. Keep local SQLite-only recovery checks in `LocalCollectRecoveryDb`.
4. Keep shadow-worker integration checks in `ShadowCollectRecoveryScenario`
   with `ScenarioRoles` and flow-local `CollectRecoveryGiven`,
   `CollectRecoveryWhen`, and `CollectRecoveryThen` traits.
5. Use `scenario.given()` for persisted stale facts, recoverable collect rows,
   and fake chain evidence.
6. Use `scenario.when()` for recover command execution in shadow-worker
   scenarios.
7. Use `scenario.then()` for chain query counts, hash backfill, confirmation
   state, and receipt-upload readiness.
8. Keep local component operations (`given_*`, `when_*`, `then_*`) on
   `LocalCollectRecoveryDb` until those cases are split into their own
   component file.
9. Keep raw SQL setup, TRON probe adapter, raw transaction JSON, scanner helper,
   worker construction, and `seed`/`load`/`count`/`assert` internals below the
   scenario layer in `collect_recovery/support.rs`.

Collect rebroadcast invalidation DB guard:

1. A collect row with `transaction_time IS NULL` and prior broadcast evidence
   may clear stale `raw_tx`, `tx_hash`, `building_at`, and broadcast-uncertain
   facts for rebuild.
2. Prior broadcast evidence includes either `last_broadcast_at` or
   `broadcast_uncertain_since_at`; TRON broadcast timeouts can persist only the
   uncertain marker.
3. Confirmed rows (`transaction_time IS NOT NULL`) must not be invalidated.

`tests/harness` remains reserved for cross-flow environment and fake
capabilities. `src/testkit` remains reserved for crate-private worker or
scanner entrypoints.

## Gaps To Close Next

- Collect TX ACK idempotency, integration:
  ACK side effect is sent once and durable ACK facts prevent duplicate sends.
- Collect receipt upload retry, integration:
  backend upload failure leaves receipt facts retryable.
- Collect notification success path, integration:
  a fresh collect order persists once and emits the expected notification.
- Move payload-only component tests closer to the receipt payload builder source
  once the source-side test layout is standardized.
## Completed Fee Cycle Shortage Closure

- Test:
  `wallet-api/src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs`
  `tron_token_collect_fee_report_does_not_complete_fee_cycle`
- Invariant: uploading the TRON service-fee request is not evidence that its fee
  transfer arrived. TRON waits for the backend fee order and `TX_FEE_RES` ACK
  before BuildTx may resume.
- Test: `service_fee_report_without_fee_order_ack_is_not_completed`
- Invariant: `service_fee_uploaded_at` means the fee transfer was requested;
  only the additional `tx_fee_res_ack_sent_at` fact completes the fee-order
  handshake.
- Tests: `sol_token_collect_fee_shortage_after_fee_cycle_terminates`,
  `evm_token_collect_fee_shortage_after_fee_cycle_terminates`,
  `evm_native_collect_fee_shortage_after_fee_cycle_terminates`, and
  `tron_native_collect_fee_shortage_after_fee_cycle_terminates`.
- Invariant (updated 2026-10-14): non-EVM completed-cycle shortages remain terminal.
  EVM completed-cycle shortages retain the single-payment facts and re-estimate
  for 180 seconds from the original fee ACK; a fresh shortage after that deadline
  is terminal. No second fee request is permitted.
- Test: `completed_fee_balance_shortage_cools_down_before_build_retry`
- Invariant (updated 2026-10-14): within the EVM re-estimation window, an
  insufficient row remains eligible after the existing 30-second scan cooldown;
  non-EVM shortages and persistent EVM shortages after expiry remain terminal.

### Collect Broadcast Guard Admission

- Test: `collect_broadcast_guard_skip_does_not_become_uncertain`
- Pre-execution skip: RPC guard/breaker admission skips mean no request reached
  the chain node, so the raw transaction remains broadcast-eligible.
- Retry: schedule a one-second point-to-point BroadcastTx wakeup without writing
  `broadcast_uncertain_since_at`.
- Invariant: only an RPC execution timeout or ambiguous RPC response may enter
  broadcast-uncertain recovery.
### Collect Service-Fee Reservation Before Resource Delegation

- Layer: unit plus integration.
- Entrypoints:

  ```text
  src/infrastructure/api_trans/resource_reservation.rs
  same_funding_address_reserves_idempotently_and_releases

  tests/integration/api_wallet/collect_resource_gate/mod.rs
  tron_collect_fee_funding_shortage_fails_before_resource_delegation
  ```

- Memory facts: reservations are keyed by `trade_no` and aggregated by chain
  and withdrawal funding address inside the single wallet process.
- Resource gate: a TRON token collect reserves its maximum native-fee shortfall
  before any platform or local resource delegation request.
- Failure: insufficient unreserved withdrawal-wallet balance records a terminal
  balance error and leaves the resource gate and dependency facts empty.
- Release: a successful build or fee-result ACK releases the reservation.
- Invariant: repeated evaluation of one order is idempotent, concurrent orders
  cannot reserve more than the observed funding balance, and no database schema
  is required.

### Collect EVM Signed Maximum Fee Admission

- Layer: unit.
- Entrypoints:

  ```text
  src/domain/api_wallet/adapter/eth_tx.rs
  metadata_free_fee_uses_propose_total_instead_of_adding_fast_total

  src/infrastructure/api_trans/collect/shadow/worker/collect_worker.rs
  evm_collect_fee_admission_uses_propose_max_fee_only
  ```

- Fee facts: metadata-free API Wallet ETH builds interpret `proposeGasPrice` as
  the total propose-tier gas price and derive priority from `suggestBaseFee`;
  `fastGasPrice` is not added as a second total price.
- Admission: ETH and BSC collect compare native balance with the propose tier's
  maximum fee, then recheck the signed maximum before persisting broadcastable
  facts.
- Failure: an insufficient signed maximum reopens the existing service-fee
  cycle and does not persist `raw_tx` or `tx_hash`.
- Invariant: ordinary-wallet fee selection and transfer paths remain unchanged,
  while API Wallet ETH and BSC collect cannot pass an estimate-only balance
  check and fail later at node admission solely because max fee was omitted.

## EVM collect nonce floor before build

- Entry: `cargo test -p wallet-api --features integration-tests --test mod collect_nonce`.
- Tests: `collect_nonce_skips_consumed_chain_nonce`,
  `collect_nonce_stale_chain_response_preserves_local_reservations`,
  `collect_nonce_rpc_failure_does_not_reserve_nonce`,
  `collect_nonce_concurrent_allocations_remain_unique`.
- Build allocation uses a fake chain adapter and real SQLite nonce repository.
- DB facts: local last-used 0 plus chain next 2 reserves 2, not consumed nonce 1;
  subsequent stale chain next 0 reserves 3, never rolling back reservations.
- External calls: each allocation queries chain nonce; RPC failure reserves
  nothing and allocation never invokes transfer. Concurrent callers reserve distinct nonces.
- Scope: new EVM collect builds only. Existing raw/hash recovery remains unchanged;
  missing receipt plus consumed pending nonce is not proof of chain failure.

## Finalized conflicting nonce: conditional database fact (batch 2a)

- Entrypoint: `cargo test -p wallet-database collect_nonce_conflict`.
- Component tests use temporary SQLite and the production repository/DAO.
- `collect_nonce_conflict_records_failure_without_replacing_transaction`:
  persist original hash/raw/nonce and finalized conflicting hash/block evidence;
  error enables receipt upload but excludes build, broadcast and recovery scans.
  No chain timestamp or finished timestamp is invented. Duplicate calls affect zero rows.
- `collect_nonce_conflict_cannot_overwrite_advanced_or_replaced_order`: confirmation,
  receipt-upload, finish, prior error, terminal status, or changed hash/nonce/sender/chain
  prevent all mutation (full row equality asserted).
- `collect_nonce_conflict_rejects_same_hash_and_malformed_evidence`: same original
  hash and malformed identity/finality metadata cannot become failure facts.
- `collect_nonce_conflict_concurrent_reports_commit_once`: concurrent attempts
  persist exactly one terminal error fact. No external calls occur in this DB API.
- Caller must independently verify different-transaction inclusion in a finalized
  block. Metadata validation here is not verification of blockchain finality.
  The API worker integration below supplies independently queried finalized evidence.


### Finalized EVM nonce conflict recovery (API batch 2b)

- `collect_recover_finalized_nonce_conflict_enters_failure_receipt_without_resend`:
  expired uncertain EVM collect + advanced nonce + different finalized transaction
  enters failure receipt scan; original raw/hash/nonce remain intact, no fabricated
  confirmation/finish/upload, and broadcast/recover scans exclude the failed row.
- `collect_recover_missing_or_unavailable_finalized_proof_preserves_original`:
  missing proof or RPC timeout leaves status/error/identity unchanged, records an
  observation retry, and does not enqueue a failure receipt or send a transaction.
- `collect_recover_confirmed_original_never_checks_nonce_conflict`: confirmed
  original follows normal receipt recovery and performs no conflict lookup.
- Adapter `eth_tx::nonce_conflict::tests`: a historical nonce search only locates
  the candidate block. Different hash + exact sender/nonce + block inclusion + a
  stable finalized anchor are required; pending-only progress, original inclusion,
  malformed/missing/mismatched results and inconsistent anchors emit no proof.
  RPC errors remain observation errors. Whole probe has a 30-second timeout and
  at most 64 historical state queries. Providers without finalized/historical or
  EIP-1898 state support leave the collect uncertain for subsequent recovery.
- All tests use fake RPC/chain boundaries and temporary local databases; no live
  transaction is broadcast, rebuilt, or retried by conflict recovery.

### Recovery confirmation hash backfill (database batch 2c)

- `collect_recover_hash_completes_missing_or_matching_identity_once`: NULL, empty,
  space-only and exact matching stored hashes accept a nonblank confirmed hash;
  chain time and receipt eligibility are persisted, raw/nonce preserved, uncertain
  observations cleared. Duplicate confirmation leaves the full row unchanged.
- `collect_recover_hash_preserves_replaced_or_confirmed_rows`: different nonempty
  hashes and already confirmed rows reject the update with full-row equality.
- `collect_recover_hash_rejects_blank_confirmation_identity`: empty/space-only
  incoming hashes cannot create confirmation facts or mutate the row.
- The existing API `collect_recover_backfills_missing_tx_hash_before_receipt_upload`
  regression remains enabled and validates worker recovery through receipt scanning.
- Validation: all 3 database regressions and all 7 API `collect_recovery` cases
  pass offline, with no skipped backfill case. The batch 2b limitation is resolved.

### Production-shaped conflict and failure receipt verification (API batch 2d)

- Finalized nonce probe trims surrounding sender whitespace before RPC and
  identity validation; `finalized_inclusion_proves_different_transaction_consumed_nonce`
  includes a padded sender matching production logs. Recovery fixtures persist
  padded senders and return canonical evidence, exercising DB identity matching.
- `collect_finalized_conflict_receipt_finishes_once_without_replacing_hash`: real
  conflict fact -> worker -> mock HTTP executeComplete sends FAIL and original
  hash with conflicting hash in remark. Upload/finish facts persist only after
  acknowledgement, repeated worker execution sends no second receipt.
- `collect_finalized_conflict_receipt_retries_backend_failure_before_finishing`:
  backend rejection retains original raw/hash/nonce and failure receipt eligibility;
  retry succeeds and ends the order without inventing chain confirmation.
- Live verification is separate: logged production RPC returns HTTP 401 without
  authentication; admin browser renders only its title. Neither establishes RPC
  capability support or actual production backend/UI acceptance.
- Recovery emits structured `collect_finalized_nonce_conflict`,
  `collect_nonce_conflict_probe_unavailable` and `collect_nonce_conflict_unresolved`
  events with order/address/hash/nonce context (and elapsed time for unresolved).
  These are log signals; no production Grafana alert rule or notification route
  has been created by this change.

- Receipt scenarios use isolated databases under target/ and a test-only Context
  without actor handles. Explicit scanner/worker execution retains strict HTTP
  request-count assertions without racing the manager startup warm scan.

### Nonce alignment safety (2026-09-07)

Shared nonce alignment no longer overwrites the allocation counter with a lower
pending observation. A local floor ahead of the chain observation produces
`nonce_gap_requires_recovery`; collect pre-build propagates this rather than
claiming successful repair and reallocating a consumed reservation. Existing
collect_recovery success, uncertain/conflict and missing-hash cases remain the
flow regression suite. Allocation interleaving tests live in transfer_nonce.
Real nonce holes may remain blocked until transaction-aware recovery is added.

### Collect-fee chain-aware allocation (batch 2c)

Collect-fee ETH/BNB get_nonce now observes chain next before atomic reservation.
BuildTx reads the fresh sender/chain under its address lock, then performs the
RPC-backed allocation outside that lock. Existing build-slot/broadcast policies
remain unchanged. Tests use the existing BNB fixture and the real fee worker's
allocation method through testkit (no signing or live sends):
- `collect_fee_nonce_syncs_chain_progress`: chain next 9 then 15 produces matching
  reservations and persisted floor, with one observation per allocation.
- `collect_fee_nonce_rpc_failure_recovers_without_reservation`: RPC failure leaves
  floor unchanged; retry observes chain and reserves 9, without broadcasting.
- `collect_fee_nonce_shared_senders_remain_unique`: concurrent fee/withdraw/collect
  allocation returns 9/10/11, with all three paths observing chain state.
Allocation-step tests do not replace full-chain emulator acceptance.

Batch 2c validation: 19 nonce integration tests pass. Expanded collect_fee suite
has 8 passes and 2 existing failures (completed fee-cycle update row count and
ETH fee upload amount); a HEAD fee-worker comparison reproduces the same failures.
No test was skipped or weakened; expanded suite acceptance remains outstanding.
See target/collect-fee-nonce-flow{,-baseline}.log.


### Nonce error semantics and broadcast visibility

- `fee_errors_are_not_consumed_nonces` distinguishes replacement underpricing,
  generic underpricing, consumed nonce and future nonce.
- `nonce_fee_rejections_preserve_reservation_without_chain_repair` verifies fee
  rejection stays an error, SQLite reservation stays at 9 and no extra nonce RPC
  or broadcast occurs even when chain next is configured to 50.
- `nonce_known_transaction_preserves_reservation_without_chain_repair` verifies
  node visibility does not advance the reservation or query chain state.
- Shared broadcast classification accepts bare `already known` with local facts;
  `unknown transaction` and fee rejection imply neither acceptance, nonce repair,
  nor raw rebuild. This does not prove receipt confirmation or implement fee bumps.
- Boundary: worker terminal/retry policy and existing nonce-too-low recovery remain
  unchanged. Historical network identity migration remains a separate batch.
- Fee contract follow-up: the previously recorded 8/2 collect_fee suite limitation
  was resolved in 5edd9c40; all 10 collect_fee tests passed.


### API Base/Scroll native fee totals (2026-10-07, narrowed candidate)
Scope: persisted raw order paths and fee display, Base/Scroll native and standard ERC20 transfers. Core efc3ccdc unchanged.
- Base/Scroll only: checked U256 execution cap + explicit L1 estimate; exact internal quote for collect funding.
- Metadata cannot bypass L1 observation or native/token balance gates on these two chains.
- Base Core estimate is an upper-bound reserve, not an actual final-fee claim.
- RPC failure/zero additional fee cannot create broadcastable raw facts; rebroadcast uses existing raw/hash/nonce without repricing.
- Base/Scroll pre-raw fee shortage reuses existing invalidation/advancer; the completed single fee cycle waits for bounded re-estimation before terminating a persistent shortage (updated 2026-10-14). Shared address lock covers fresh-read through invalidation, serialized with the sole Build raw commit; no cross-process DAO CAS claim.
- ETH/BNB/Polygon/Avalanche/Fantom/Arbitrum retain baseline balance/gas/oracle order, metadata raw behavior, display/snapshot and testnet direct transfer paths.
- Removed broad snapshot override and direct transfer rewrite. Snapshot uses the original display-response estimate path; its existing floating display conversion/DB dependency remains. Direct transfer is baseline even for Base/Scroll; this batch does not claim every API transfer entrypoint has L1 funding protection.
- Generic old metadata balance bypass and fee precision work are deferred, not fixed for unrelated chains.
- Entrypoints: adapter/eth_tx/fee_tests.rs (api_evm_fee_*); collect/shadow/worker/fee_recovery_tests.rs.
- Shared calculate_fee_setting_from_req and build_fee_setting fully match baseline. Base/Scroll direct transfer no/invalid metadata retains old amount gate; valid metadata retains old shortcut. Only the new exact funding quote uses an isolated l1_quote_fee_setting without local balance gates. Raw executes old amount gate followed by the new total fee gate.
- Isolation regression: compile-only failure is not RED; gas-isolation-red-behavior.log confirms real missing Base native direct-transfer amount gate. Final isolated-version evidence: gas-isolation-final-0.log has 15 passed; final-1..5 freshly pass trans 75, EthTx 5, withdraw shadow 66, collect_fee shadow 21, collect worker 56 (223 total). Independent full-callee review found no blocker. Earlier 12+223 is superseded. Full order orchestration, live nodes and App acceptance remain unrun.

## Single-payment EVM collect funding repair (2026-10-14)

- `fee_quote_funding_and_build_use_same_l1_inclusive_amount`: both real worker estimate entrypoints use the same exact native wei quote for Base/Scroll, including separate L1; display parsing is not called.
- `fee_quote_l1_error_does_not_fall_back_to_underfunded_execution_quote`: failed L1 RPC must propagate, never request a gas-only payment.
- `fee_quote_arbitrum_preserves_exact_execution_maximum`: other EVM quote calculation retains propose gas maximum.
- `fee_quote_headroom_covers_logged_arbitrum_increase_and_rounds_up_wei`: first funding budget includes 20% headroom rounded up in wei; build admission still uses fresh unbuffered quote.
- `collect_eth_service_fee_upload_uses_exact_propose_maximum`: captured backend payload uses exact quote plus funding headroom; SOL upload tests retain unchanged rent behavior.
- `api_evm_fee_completed_cycle_waits_for_reestimate_without_second_payment` and `api_evm_fee_duplicate_shortage_preserves_ack_deadline_and_scan_recovery`: shortage remains nonterminal, original fee upload/ACK retained, build slot released, no immediate retry/payment intent, existing scanner discovers the order again after its 30-second interval.
- `api_evm_fee_reestimate_deadline_expires_without_second_payment` and `evm_fee_reestimate_deadline_uses_original_ack_time`: 180-second deadline uses original ACK, not retry/update time, and survives restart through existing DB facts.
- `api_evm_fee_stale_shortage_preserves_committed_raw_under_shared_lock`: stale fee failures cannot discard committed raw/hash/nonce.
- Limitation: one payment cannot guarantee arbitrary fee increases; beyond the funded budget, wait for lower fresh quotes, then report persistent insufficiency at the deadline. No real transactions or historical failed-order replay in this repair.

- `api_evm_fee_stale_build_preserves_pending_single_payment`: stale direct fee-check and adapter-error recovery protect the uploaded fee while awaiting FeeRes or its ACK, including `need_service_fee=true/false`; neither clears upload facts nor requests another payment.

### Collect-fee signed nonce candidate (2026-10-14)

Build slot precedes nonce RPC; EVM observation reserves nothing. Signed fee facts,
derived status and nonce are committed together under the shared SQLite counter
writer lock. Existing signed facts cannot be replaced. Non-EVM build persistence
and broadcast/receipt/ACK policies remain unchanged.

- `fee_signed_candidate_success_commits_raw_status_and_nonce_once`: real SQLite
  stores raw/hash/nonce/status atomically; identical retry no-op, changed bytes rejected.
- `fee_signed_candidate_competing_allocator_leaves_no_raw`: occupied nonce cannot
  publish signed bytes or change the existing counter.
- `fee_signed_candidate_stale_lifecycle_and_identity_roll_back_nonce`: sender
  mismatch/finished row rejects publication and rolls back provisional floor.
- `fee_signed_candidate_result_ack_terminal_rolls_back_nonce`: a historical
  result-ACK-only terminal row rejects publication and leaves no reservation.
- `fee_signed_candidate_concurrent_same_sender_commits_only_one`: competing fee
  rows for one candidate commit exactly one signed intent.
- `collect_fee_build_failure_retries_same_nonce_then_commits`: adapter build
  failure leaves no raw/reservation; retries succeed with the same chain candidate;
  no broadcast and no duplicate build once bytes exist.
- `collect_fee_build_occupied_slot_skips_nonce_and_build`: duplicate occupied slot
  performs zero nonce/build RPC and reserves nothing.
- `collect_fee_build_cancelled_before_commit_reserves_nothing`: controlled blocked
  build cancellation leaves only a reclaimable slot, no raw or nonce allocation.
- `collect_fee_build_competing_allocator_discards_candidate`: reservation changes
  while the adapter is blocked; stale signed candidate is not published/broadcast.
- Existing `collect_fee_nonce_*` cases now assert candidate observation is read-only,
  RPC failure leaves no reservation, and occupied counters require gap recovery.

Runtime incident and conditional recovery boundaries:
[Arbitrum nonce recovery](../arbitrum-collect-fee-nonce-recovery.md). Existing signed
high-nonce intents are not reset or rewritten. No real-device recovery was run.

## Collect-fee payer shortage becomes terminal (2026-10-14)

Scope: common fee BuildTx failure only. A typed payer InsufficientBalance or
InsufficientFeeBalance before any raw/hash/broadcast/result evidence writes
BalanceInsufficient/FeeInsufficient, clears building_at, and stays reportable.
No new parent mapping, payment request, backend endpoint or migration.
Temporary build errors retain existing retries. Recipient rent errors retain
their existing handling. Post-broadcast/recover paths are unchanged.

- `collect_fee_payer_shortage_stops_build_after_later_funding`: fake adapter
  returns payer shortage; DB freezes the child, scanners no longer select
  BuildTx but select failure upload; removing shortage and replaying build does
  not query nonce, build, reserve, or broadcast another transaction.
- `collect_fee_build_timeout_retries_without_terminal_failure`: timeout leaves
  no terminal error/raw/nonce reservation, releases build slot, and next build
  succeeds when the fake network recovers.
- `collect_fee_shortage_reports_failure_and_never_executes_again`: actual
  receipt worker sends one mock backend COL_FEE/FAIL receipt without hash and
  finishes only after upload succeeds; repeated upload and stale Build/Broadcast
  commands are silent.
- `collect_fee_shortage_retries_only_failed_receipt_upload`: failed mock upload
  leaves durable error, no execution eligibility and no finished_at; new worker
  against persisted DB cannot execute, but retry upload succeeds and closes.
- `collect_fee_shortage_freezes_inflight_build_before_nonce_commit`: controlled
  adapter timing lets failure win before candidate commit; neither raw nor
  nonce reservation can appear afterwards.
- `collect_fee_recipient_rent_shortage_keeps_existing_retry`: recipient rent is
  not misclassified as payer shortage and remains build-eligible.
- Database `fee_build_failure_*`: real SQLite proves idempotency, duplicate
  order preservation, failure upload eligibility, legacy/EVM late-commit
  rejection, and protection of concurrently committed/broadcast/uncertain/
  confirmed/result/finished facts.

Commands: `CARGO_TARGET_DIR=target cargo test -p wallet-api --test mod
transfer_nonce::collect_fee_ --offline -- --test-threads=1`; `CARGO_TARGET_DIR=target cargo test
-p wallet-database --lib fee_build_failure --offline`. Existing
`fee_signed_candidate` and fee receipt scanner cases cover preserved successful
nonce commits and pending/success/failure receipt conditions. Real-device
funding/retry is not executed by this task; rebuild is required for adoption.


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

### Main EVM build nonce reservation (2026-10-16)
- A failed/timed-out/cancelled build publishes no signed facts and reserves no nonce;
  retry observes the same chain candidate unless another valid sender consumed it.
- Signed raw/hash/fee/status and api_nonce commit in one SQLite transaction;
  competing allocations, stale lifecycle and partial write failures roll back.
- A duplicate build/restart reuses durable signed facts without reallocation.
- Existing nonce gaps stop new builds; no automatic rollback or historical repair.
- Evidence: transfer_nonce::collect_main_* integration cases and
  collect_signed_candidate component cases (target/collect-main-nonce-fix logs).

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

## Verified unsigned nonce gap report (2026-10-18)
- Missing receipt after existing uncertainty timeout plus chain next below order
  nonce: atomically verify shared signed claims and fail only if an unsigned
  hole remains. Existing normal queued lower signed orders prevent failure.
- Preserve raw/hash/nonce/counter; failure receipt eligible, no recovery or
  broadcast retries, idempotent report fact. Confirmed/RPC-uncertain untouched.
- Shared allocator may fill earlier unsigned hole but cannot reuse or advance
  past the reported signed slot until chain frontier consumes it.
- Evidence: collect_nonce_gap* database tests and collect_recover_nonce_gap*
  mock integration; no live transaction or backend deployment verification.

## QueryUncertain nonce gap (2026-10-18)
- QueryUncertain may independently verify a signed EVM gap after the existing
  five-minute age; use legacy last_broadcast_at when uncertainty marker absent.
- Nonce query failure/recent broadcast/no gap keep original signed facts and
  do not upload failure; no compensating broadcast or cancellation.
- Shared allocator can fill a gap bounded above by durable signed facts even
  when legacy counter153 exceeds signed6. Bare counter and unknown trailing
  slots remain guarded; counter never rewound, reported signed slot retained.
- Mock integration collect_query_uncertain* plus DB legacy-counter and pure
  decision tests cover report eligibility and actual new5 candidate commit.

### Explicit Scroll test-address replacement (2026-10-19)

- Default disabled; dev/non-prod explicit scope selects old order/hash/sender/nonce/counter bound. Existing signed relations survive restart.
- `replacement_signed_candidate_preserves_old_and_commits_relation`, `replacement_concurrent_preparation_has_one_owner`: signed-first atomic linkage, immutable old raw, one active owner.
- `replacement_checkpoint_allows_actual_seven_and_fences_legacy_reservation`: confirmed slot consumption enables real signed7, retains153 audit, blocks legacy counter-first increments while unresolved, and permits verified external chain-frontier advancement after resolution.
- `replacement_relation_survives_reopen`, `replacement_migration_preserves_legacy_signed_rows`: restart and existing data preservation.
- `replacement_winner_write_failure_rolls_back_confirmation_and_checkpoint`: winner and new success fact commit together.
- `replacement_padded_legacy_sender_is_rejected_before_preparation`: reject ambiguous whitespace aliases before ownership selection.
- `replacement_underpriced_reports_requote_but_keeps_signed_watch`: report explicit quote failure, never overwrite signatures; continue result observation.
- API `collect_replacement_reconciles_each_winner_and_continues_from_seven`: old/new winners × success/revert; duplicate recovery stable; actual receipt fee/resource retained; old hash never marks new success.
- Adapter `replacement_receipt_checks_winner_identity_canonical_block_and_revert`: receipt/transaction/canonical block/finalized anchor checks, changed anchor and wrong block height rejected; Scroll gasUsed × effectiveGasPrice + l1Fee replaces budget.
- Ordinary `collect_main_` and database `nonce_` regressions verify failed/cancelled builds remain non-reserving and shared ownership protections remain.
- Uncovered: live gateway replacement acceptance/finality availability, App packaging/deployment, actual transaction signatures/broadcast. No running config or device DB mutation in this stage.

### Scoped replacement fee preflight and confirmed frontier (2026-10-19 follow-up)

- `collect_replacement_pending_seven_latest_six_still_replaces_six`: node pending7/latest6 still builds nonce6; strict quote and build retain replacement metadata and original signed-slot authority.
- `collect_replacement_fee_preflight_receives_replacement_before_quote`: before fix, regular quote rejected pending-deducted funds; replacement scope now prepared before quote. No balance override or transfer value reduction.
- `collect_replacement_unknown_latest_or_unproven_consumption_never_signs`: latest RPC error or latest7 without canonical winner never produces raw/hash, marks a winner, or moves counter153.
- `collect_replacement_true_fee_shortage_reports_failure_before_signing`: actual quote over latest funds reports FeeInsufficient, abandons only unsigned attempt, never builds/signs or requests automatic fee top-up.
- Adapter `replacement_estimation_uses_latest_original_value_and_fixed_nonce`: RPC request is eth_estimateGas(original tx with authorized nonce, latest), retains value/from and 10% checked gas buffer. Same helper is used by quote and raw build, including the non-mainnet branch.
- Adapter `replacement_latest_reads_do_not_fallback_or_accept_overflow`: latest-only transaction count, no pending fallback, RPC errors/overflow rejected.
- Live evidence (read-only, separate from offline tests): target/collect-replacement-preflight/readonly-estimate-facts.json records same actual nonce6/value failing default estimate but succeeding with latest (21000 gas), while latest6/pending7; no transaction was signed/broadcast.
- Existing winner reconciliation and ordinary collect regressions are retained. Scope remains opt-in dev test address; no runtime config/device DB mutation, build deployment or real signing/broadcast in this follow-up.
- Review correction: `collect_replacement_retained_scope_allows_next_real_seven` keeps the explicit scope configured, resolves the signed replacement, then creates and actually commits the next collect at7. `replacement_retained_scope_is_released_only_by_matching_verified_resolution` requires the exact old hash/address/nonce/bound and verified resolution before retiring the old-slot selection; mismatched scopes remain inactive as checkpoints. Common allocation still checks signed claims and unknown counters.

### Automatic reported-gap replacement discovery (2026-10-19)

- Lifecycle: build slot → fee preflight → signed ownership → recovery. No schema/new queue/engine. Native EVM collect discovery uses the same sender/chain/UID/asset/destination, reported gap failure with uploaded receipt, unique old signature, and unsigned live new order. Discovery alone reserves nothing; prepare atomically rechecks all three flows and records the observed counter bound.
- `replacement_discovery_uses_persisted_reported_intent_and_counter_without_configuration`: failure upload required; derives old identity and153 from DB, commits new6 without erasing old signature or counter.
- `replacement_discovery_rejects_other_destination_and_ambiguous_old_intents`: mismatched target or multiple failed intents cannot create a replacement association.
- `replacement_discovery_does_not_reselect_canonically_resolved_old_intent`: canonical winner retires discovery; future order cannot reselect6.
- `collect_replacement_automatically_detects_failed_old_slot_without_configuration`: no configured scope or preexisting relation, latest6/pending7; ordinary worker prepares before quote, retries sign once, winner reconciles and next order actually commits7.
- `collect_replacement_automatic_discovery_never_signs_on_unknown_latest_or_winner`: RPC failure or advanced nonce without canonical winner produces no signature or counter release.
- `replacement_quote_uses_exact_native_budget_on_every_evm_chain`: Ethereum/BNB/Base/Scroll replacement metadata routes to native integer quote, never ordinary pending-state display quote. Existing quote helpers retain non-replacement behavior and L1 fee budgets.
- Preserved evidence: existing replacement winner/failure/restart/foreign-flow/counter regressions plus ordinary collect build-failure/cancellation tests. No real signing/broadcast or device mutation. Live read-only reproduction saved in target/collect-automatic-recovery/runtime and is not an offline test dependency.
- Not covered by this change: automatic replacement of token collects, withdrawals or fee intents; their existing shared signed-first nonce allocation and ownership protections are unchanged. Current transaction acceptance and actual phone deployment require separate runtime verification.

- Review follow-up: canonical receipt fee proof requires l1Fee on Base/Scroll; ordinary EVM receipts use execution fee without requiring a nonexistent l1Fee. Extended `replacement_receipt_checks_winner_identity_canonical_block_and_revert` covers both fee models and missing required L1 evidence.
- `replacement_raw_rechecks_native_maximum_budget_on_non_l1_chains`: real adapter + localhost fake RPC, synthetic identity; latest balance covering only value rejects raw construction, funded path builds, neither broadcasts. Ordinary non-L1 metadata behavior remains covered by `api_evm_fee_non_l1_chains_keep_metadata_raw_without_new_balance_checks`.
- `replacement_automatic_checkpoint_preserves_higher_signed_or_uncertain_ownership`: recovered legacy bound does not remove signed or uncertain7; signed or broadcast-uncertain7 stays owned; the next unsigned slot is8 and7 is never reused. Current three flows persist signed facts atomically before raising the counter; no active counter-first caller was found. Historical external signatures cannot be disproved from local rows; legacy counter-first writers remain unsupported during the recovery fence.

### Android debug command stack boundary (2026-10-19)

- Trigger: native arm64/16KB Android SIGSEGV during replacement estimate HTTP connection before signing. Symbolized8761cf75 debug prologues: execute_intent≈749KB and handle≈392KB, plus build and RPC frames. This is cumulative async poll-frame amplification, not evidence of recursive nonce recovery or RPC transaction failure.
- `collect_mobile_stack_dispatch_future_is_bounded`: real dispatcher Future must stay below16KiB so command states do not grow every branch; regression measured43312 bytes before correction.
- `collect_mobile_stack_command_future_is_heap_bounded`: command Future must stay below4KiB; regression measured43168 bytes before correction. Worker command body is unchanged; a synchronous boxed-future boundary prevents caller embedding, with no spawn, thread-stack inflation or detached operation.
- Existing collect_replacement_ integration scenarios cover unsigned relation/resume, duplicate commands, fee failure/unknown latest safety, canonical winner and next7. Existing collect_main_ preserves failure/cancellation non-reservation. Android validation runs standalone isolated test binaries with temporary SQLite and localhost fake RPC; real App, credentials, order DB and chain execution are excluded.
- Runtime audit before implementation: latest new collect has no raw/hash/broadcast facts; relation unsigned; crash PC precedes raw signing. Old6 remains independently pending. No new fee/withdraw signature or broadcast in the captured session. Evidence under target/android-stack-crash is local diagnostic output, not a default live test dependency.

### Normal receipt confirmation for linked replacement and captured-counter recovery

- User rejected the uncommitted backup-node mechanism and the extra L1-finalized gate. Known old/new signed identities follow normal collect included-receipt semantics using the selected RPC only; no finalized-tag request, no node lookup/failover.
- `replacement_receipt_uses_normal_included_result_without_finalized_tag`: real adapter + localhost fake selected RPC with valid nonce6 receipt and HTTP500 on finalized; resolves success with exact fee and zero finalized/chainId calls. `replacement_receipt_rejects_changed_canonical_block_on_selected_rpc` and canonical table cover mismatched nonce/sender/block, reorg during read, revert, invalid status and fee fields.
- Missing/uncertain receipts never prove consumption. The separate existing finalized proof for unknown external nonce conflicts remains unchanged. Known winner receipt must verify exact hash/from/nonce/block and canonical membership/reread; backend confirmation semantics stay aligned with ordinary collect.
- Resolve winner, recovered execution facts and captured last-used counter normalization atomically (consumed6 -> stored6 -> next candidate7). Preserve original153 audit, higher signed/uncertain ownership and unknown counter growth; duplicate resolution cannot overwrite later work.
- `collect_replacement_included_winner_reports_once_and_restores_next_seven`: scanner/dispatcher uploads one SUCCESS using new6 identity after idempotent reconciliation. `collect_replacement_releases_ordinary_seven_without_virtual_reservation`: actual ordinary observer returns7 without reserving; synthetic signed commit then owns7; no real signing/broadcast.


### 2026-10-19 自动恢复不依赖指定测试范围

- `replacement_legacy_scope_cannot_select_chain_address_or_nonce`：旧配置字段不再参与选择，eth/scroll/base的遗留值均按未知配置忽略。
- `replacement_discovery_and_resolution_use_dynamic_identity_and_nonce`：临时SQLite验证eth/base/arbitrum、不同发送地址、nonce 1/42/1001和独立历史高水位9/97/4096；自动发现、唯一占用、签名关联、赢家对账后最后使用N/下一N+1、重复对账幂等，以及其他地址计数器不变。
- `collect_replacement_restart_uses_durable_relation_then_normal_next_nonce`：上下文重建只依赖持久化关联，无手动配置入口；恢复完成后下一归集走正常分配。已有`collect_replacement_`失败/未知结果/并发占用/上报回归继续适用。
- 业务边界：公共分配与签名占用覆盖归集/手续费/提币；自动关联替换只覆盖原生EVM归集，不扩展其他业务自动重签或取消权限。


### 2026-10-20 禁止未签名预分配nonce

- `ApiNonceRepo`与`NonceEngine`的compile_fail文档用例：旧公开预分配方法不存在；DAO不再包含对应counter+1 SQL。
- `shared_candidate_nonce_skips_consumed_without_reserving`、`shared_candidate_nonce_stale_chain_response_does_not_advance_counter`、`shared_candidate_nonce_concurrent_observations_do_not_reserve`：替代仅验证旧分配器的历史用例；重复/并发观察不写nonce，过时链前沿不能提高counter或绕过不明占用。
- `nonce_alignment_preserves_reserved_nonces`、`nonce_alignment_and_observation_preserve_reservations`：对已存在高水位重复或并发自愈不能回退/继续加号；缺少签名事实时保持保守等待。
- 既有`candidate_nonce_`、`collect_main_`、`collect_fee_build_`、`collect_replacement_`及withdraw用例继续覆盖实际构建失败/取消、签名提交竞争、重启、未知广播与归集替换恢复。竞争写入夹具直接注入counter变化，不再调用已删除的预分配API。


### 2026-10-20 EVM身份与历史恢复隔离

- `self_transfer_case_alias_cannot_commit_a_second_nonce_owner`、`self_transfer_case_alias_concurrent_commit_has_one_owner`：大小写别名不能顺序或并发保存第二个同nonce签名占用。
- `self_transfer_case_alias_cannot_ignore_signed_claim_without_counter`、`self_transfer_case_alias_respects_collect_signed_owner`：计数器缺失也不能绕过签名事实；归集与普通转账共用占用身份。
- `nonce_identity_merges_historical_evm_alias_floors_only`、`nonce_identity_matches_sql_ascii_normalization`：历史EVM别名采用最大floor；Rust/SQL字符规则一致，非EVM大小写行为不变。
- `completed_recovery_never_ignores_a_new_unknown_counter`及扩展`replacement_winner_recovers_counter_only_after_chain_consumption`：构造和真实resolve两种完成路径后，新的未知floor即使低于旧上限也应阻塞；重复resolve不改后续floor。
- `replacement_automatic_checkpoint_preserves_higher_signed_or_uncertain_ownership`：在当次事务把物理floor纠正到真实最高占用，而历史值仅留审计；更高签名和缺raw不确定记录均保留。
- `nonce_identity_gate_and_freeze_share_evm_case_alias`与既有分流程nonce回归：统一门锁、冻结、只读候选、事务占用、失败不加号、重启/替换/上报。采用隔离假链与SQLite，不启动实际钱包App。

2026-10-20 旧聚合nonce入口清理：归集/手续费/提币DAO和仓储不再提供update_tx_status_nonce。公共reserve_signed_*继续原子提交签名与计数；重复同一手续费签名提交返回0且counter不增长，不存在订单提交失败且counter与现有订单raw保持不变（fee_signed_candidate测试）。已存在的并发同地址签名只允许一个提交、生命周期或身份不符回滚继续覆盖。

### API Base L1 margin parity (2026-10-21)

- `api_base_manual_quote_reserves_l1_margin_without_changing_estimate_or_settings`: real API quote + migrated local core price/rate data + fake RPC; native/ERC20 maximum includes ceil(L1/5) only on Base, raw estimate and execution settings unchanged, spendAll=false; L1 failure returns an error, never a quote without L1. Scroll/Ethereum preserve prior totals.
- `api_base_automatic_funding_keeps_single_margin_and_rechecks_live_l1`: real balance-free exact quote and raw builder, native/ERC20. Existing one-time funding budget covers observed L1 rise; one wei beyond total budget fails before signing; returned raw fee is fresh exact execution+L1, with unchanged requested amount and no broadcast. This is shared adapter evidence for collect and fee top-up build, not a new complete backend payment lifecycle test.
- Existing `fee_quote_headroom_covers_logged_arbitrum_increase_and_rounds_up_wei` now asserts the exact Base budget used in the raw-builder regression. `fee_quote_funding_and_build_use_same_l1_inclusive_amount` preserves real funding/build worker selection of exact integer quote, not UI maximum. `fee_quote_l1_error_does_not_fall_back_to_underfunded_execution_quote` covers both Base and Scroll.
- Production change is only API display quote; automatic collect funding already has 20% total-fee headroom. Preserve fresh unbuffered build admission, signed raw fee accounting, fee top-up amount, all nonce/slot/ACK/recovery facts and backend one-payment protocol. No additional async call/shared-state mutation introduced.
