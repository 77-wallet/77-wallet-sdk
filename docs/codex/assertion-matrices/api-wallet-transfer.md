# API Wallet Transfer Assertion Matrix

Scope: `wallet-api` API wallet transfer flow.

Rules source: `docs/codex/testing.md` and `docs/codex/testing-strategy.md`.

## Current Standard Tests

### Transfer Nonce Lock Serializes Same Address

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/transfer_nonce/mod.rs
  api_wallet_transfer_nonce_lock_keeps_same_address_requests_serial
  ```

- Backend: none; the chain path uses a fake BNB transaction adapter.
- DB facts: BNB nonce floor starts at `0` for the fixture wallet.
- Chain calls: first transfer records nonce `1`; second transfer stays blocked
  until the first transfer is released, then records nonce `2`.
- Result: returned transaction hashes encode nonce `1` and nonce `2`.
- Invariant: transfers from the same address must reserve and use nonces
  serially under concurrency.

### Transfer Failure Keeps Reserved Nonce

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/transfer_nonce/mod.rs
  api_wallet_transfer_nonce_failure_keeps_reserved_nonce
  ```

- Backend: none; the chain path uses a fake BNB transaction adapter that fails
  transfer.
- DB facts: after the simulated transfer failure, nonce floor remains advanced
  to `1`.
- Chain calls: fake adapter records exactly nonce `1`.
- Invariant: a failed transfer must not rewind the reserved nonce and make it
  reusable.

## Template Contract

`transfer_nonce/mod.rs` uses the role-trait Given-When-Then shape:

1. Keep transfer nonce cases in `transfer_nonce/mod.rs`.
2. Keep fake chain adapter state, fixture address, and adapter guard in
   `TransferNonceScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep BNB fixture setup, fake chain adapter behavior, and cached wallet
   password in the flow-local `TransferNonceGiven` trait.
5. Keep transfer entrypoints and explicit release of the blocked first transfer
   in the flow-local `TransferNonceWhen` trait.
6. Keep recorded nonce order, result hashes, error text, and persisted nonce
   floor assertions in the flow-local `TransferNonceThen` trait.
7. Keep seed/load/assert internals behind harness role containers and below the
   Given-When-Then test body.
8. Keep adapter override, repository setup, and transfer request construction
   details below the scenario layer in `transfer_nonce/support`.

`tests/harness` remains reserved for cross-flow environment and fake
capabilities. `src/testkit` remains reserved for crate-private worker or
scanner entrypoints.

## Gaps To Close Next

- Transfer nonce multi-address concurrency:
  different sender addresses should not block each other.
- Transfer idempotency:
  retry behavior should not double-send a transaction after a successful chain
  response.

## API adapter factory routing during 2.2 integration

- Layer: offline unit tests in `domain::api_wallet::adapter_factory::tests`.
- Entrypoints: `adapter_kind_routes_all_evm_chains_to_eth` and
  `adapter_kind_preserves_non_evm_routes`.
- Invariant: all currently supported EVM chains use the shared `EthTx` kind;
  Tron, Bitcoin, Solana, Litecoin, Dogecoin, Sui and Ton retain their own kinds.
  An unsupported future `ChainCode` maps to no kind: startup pre-initialization
  skips it and an explicit adapter request returns `ServiceError::Parameter`
  before querying a node, instead of panicking. The current Core enum has no
  unsupported constructible variant, so that future-only branch is not
  exercised by these unit tests.

## Added EVM chain offline signing matrix

- Layer: component-style unit test with a process-local fake JSON-RPC endpoint.
- Entrypoint:

  ```text
  domain::api_wallet::adapter::eth_tx::tests::added_evm_chains_build_signed_transfer_offline
  ```

- Matrix: Polygon, Avalanche, Fantom, Arbitrum, Base and Scroll.
- Input: native transfer with explicit fee metadata and a runtime-generated test key.
- RPC boundary: exactly one `eth_chainId` request per chain; no backend, public RPC,
  balance lookup, fee lookup or broadcast.
- Result: signed EIP-1559 raw bytes are non-empty and the returned transaction hash
  equals local `keccak(raw)` for every chain.
- Invariant: every newly supported EVM chain can reach the SDK's shared offline
  transaction construction and signing path without chain-specific adapters.

## Added EVM chain read-only live-node smoke

- Layer: ignored manual smoke test.
- Entrypoint:

  ```text
  smoke::transactions::evm_added::added_evm_chains_read_live_nodes_without_funds
  ```

- Matrix: Polygon, Avalanche, Fantom, Arbitrum, Base and Scroll mainnet nodes.
- Calls: `eth_chainId`, `eth_blockNumber`, `eth_gasPrice`, `eth_getBalance` and
  `eth_getTransactionCount` only.
- Result: each node reports the expected chain ID, a positive block height and gas
  price, and valid hex quantities for a fixed read-only address.
- Safety: no wallet credentials, backend calls, transaction broadcast or on-chain
  state changes. RPC URLs can be overridden with `WALLET_SMOKE_*_RPC` variables.

## Nonce alignment reservation safety (2026-09-07)

- `nonce_alignment_preserves_reserved_nonces`: reserve 9, 10, 11; an RPC next
  nonce of 9 must not rewind the floor or reissue 9. Alignment reports
  `nonce_gap_requires_recovery`; subsequent allocation returns 12.
- `nonce_alignment_and_allocation_preserve_reservations`: concurrent alignment
  and allocation preserve outstanding reservation 9 and allocate 10, then 11.
- `nonce_alignment_advances_floor_without_reserving`: upward chain progress
  advances the last-used floor to next-1, leaving the next nonce available.
- `nonce_alignment_rpc_failure_preserves_floor`: failed observation does not
  mutate allocation state or send a transaction.
- `nonce_alignment_frozen_address_does_not_query_or_mutate`: frozen alignment
  fails before RPC/DB mutation. The test releases its manual freeze afterward.
- All cases use the existing fake transaction adapter and real nonce repository;
  assert persisted floor and RPC count. Signed-transaction recovery and automatic
  gap filling are not claimed by these allocator tests.

## Ordinary EVM transfer chain-aware allocation (batch 2a)

The ordinary ETH/BNB transfer entrypoint now queries chain next nonce before
atomic allocation, within its existing per-address transfer gate.
- `ordinary_transfer_nonce_syncs_external_chain_progress`: with local last=0,
  real transfer orchestration passes nonce 9 then 15 to the fake chain sender as
  external chain state advances; verifies returned hashes, sender calls and DB floor.
- `ordinary_transfer_nonce_rpc_failure_recovers_without_sending`: nonce RPC error
  leaves the floor unchanged and sends nothing; a later successful observation
  permits the same transfer path to send with nonce 9, proving the gate releases.
- Existing same-address serialization and post-allocation transfer-failure tests
  remain enabled. This batch does not change withdrawal/collect-fee entrypoints,
  address normalization, replacement policy or reservation release semantics.


## Ordinary transfer gap protection (2026-09-08, batch 1)

- `ordinary_transfer_uncertain_nonce_blocks_next_allocation`: fake submission of
  nonce 4 fails, chain pending next stays 4. A repeated ordinary call returns
  `nonce_gap_requires_recovery`; nonce floor remains 4 and there is only one send.
  When observation advances to 5, another submission succeeds with nonce 5.
  This is allocation recovery, not proof that the original transaction succeeded.
- `ordinary_transfer_timeout_blocks_next_allocation`: adapter timeout preserves
  nonce reservation 4 and prevents another send/allocation while node stays at 4.
- Existing same-address serialization test now models node pending advancing on
  accepted sends. Both nonce 1 and nonce 2 are still returned in order.
- Existing collect/withdraw/fee queue tests retain their allocation policy and
  unique reservations. The gap guard applies only to ordinary ETH/BNB transfers.
- A concurrent reservation between observation and allocation also fails closed
  if the resulting nonce differs from the observation; its reservation is kept.

Validation: `cargo test -p wallet-api --test mod --features integration-tests transfer_nonce`.
Offline fake chain and real SQLite; no production transaction or migration.
A stale node may conservatively block a legitimate next transfer. Signed raw/hash
persistence, restart recovery, identical-raw retry and atomic cross-flow allocation
are separate batches. This change does not repair an existing nonce gap.


## SelfWithdraw durable facts (2026-09-08, database batch 2)

Component entrypoint: `wallet-database/src/repositories/api_wallet/withdraw/self_transfer_tests.rs`.
The test module is attached to `ApiWithdrawRepo`. `SignedSelfTransfer` lives in
`entities/api_withdraw.rs`; SQL lives in `ApiWithdrawDao`;
`persist_signed_self_transfer`, `mark_self_transfer_accepted` and
`pending_self_transfers_for_sender` are repository entrypoints. Transaction
coordination and identity validation remain in the repository. These methods
do not invoke the backend withdrawal status recomputation. There is no separate
`self_transfer` production module.
Real temporary SQLite with migrations, no backend or RPC calls.

- `self_transfer_survives_reopen_and_acceptance_is_not_confirmation`: signed
  nonce/hash/raw survive pool close and reopen; pre-send uncertainty exists.
  Backend withdrawal broadcast/recovery scans exclude the ordinary record.
  Only matching-hash acceptance clears uncertainty, without confirmation or ACK.
  Repeating persistence after acceptance does not restore uncertainty.
- `self_transfer_conflicting_retry_preserves_original_and_nonce_floor`: changed
  bytes/nonce under the same trade number are rejected. Identical retry preserves
  a higher nonce floor instead of rewinding it.
- `self_transfer_nonce_write_failure_rolls_back_signed_record`: failure injected
  after the record insert rolls back the complete transaction and leaves the old
  counter intact. A later retry succeeds after the failure is removed.
- `self_transfer_isolates_sender_chain_trade_type_and_terminal_rows`: pending
  lookup and acceptance cannot advance backend withdrawals or terminal records;
  sender and chain filters isolate ordinary work.
- `self_transfer_bnb_zero_nonce_and_invalid_input`: BNB nonce zero is valid;
  non-EVM/negative nonce/empty raw are rejected without reserving a nonce.
- `self_transfer_concurrent_identical_retries_write_one_record`: duplicate
  concurrent persistence creates one durable intent and does not increment nonce.

Command: `cargo test -p wallet-database --lib self_transfer`.
No migration and no API wiring in this batch. The caller must provide a stable
trade number and validated signed payload before broadcast. The early uncertainty
marker is conservative: it does not assert that an RPC was sent. Pending lookup
returns signed, unfinished records; reconciliation policy remains a later batch.


## Ordinary EVM API durable broadcast (batch 3)

The ordinary ETH/BNB entrypoint now builds signed raw, calls
`ApiWithdrawRepo::persist_signed_self_transfer`, then broadcasts that same raw.
It marks node acceptance only after visibility verification and matching hash.
The existing signed builder stores the EIP-1559 maximum fee budget; this is not
an actual receipt fee. Non-EVM transfer paths keep their existing implementation.

- Existing same-address serialization case reads hash/nonce/raw/uncertainty via
  a separately opened SQLite pool while the first fake broadcast is blocked.
  Successful calls retain raw, clear uncertainty and set only node acceptance.
- Existing failure/recovery and timeout cases now also assert exact durable raw
  and hash. Chain nonce progress allows a new send without marking the earlier
  uncertain transaction confirmed.
- `ordinary_transfer_signed_persistence_failure_never_sends`: signed insert
  failure rolls back the candidate reservation and leaves the preexisting floor
  unchanged. Repeating the call reaches persistence again; no signed row or
  broadcast exists. The observed next nonce is never written before construction.

- `ordinary_transfer_invisible_submission_stays_uncertain`: a returned hash
  without node visibility preserves uncertainty rather than reporting success.
- `ordinary_transfer_wrong_response_hash_cannot_mark_accepted`: mismatched hash
  does not replace the local hash or mark node acceptance.
- `ordinary_transfer_acceptance_write_failure_preserves_signed_transaction`:
  a DB failure after node acceptance retains raw and uncertainty for reconciliation.

Validation: `cargo test -p wallet-api --test mod --features integration-tests transfer_nonce`.
At this historical checkpoint, no automatic rebroadcast or recovery worker was enabled; see the later observer and rebroadcast batches below. A failure before signed facts
are persisted can still leave a reserved nonce with no recoverable raw, and must
not be auto-released. Tests use a fake BNB adapter and real SQLite; full process
restart and real ETH/BNB RPC acceptance remain separate acceptance work.


## SelfWithdraw observed receipt completion (database batch 4)

`SelfTransferReceipt` belongs to entities; `ApiWithdrawDao` applies the guarded
receipt update; `ApiWithdrawRepo::confirm_self_transfer_receipt` wraps receipt
and existing `mark_chain_finished` writes in one transaction. No backend status
recomputation or ACK is performed. Only signed SelfWithdraw ETH/BNB rows matching
trade number, hash and chain with no prior receipt/terminal/error fact qualify.
The caller must supply an actually observed receipt; absence/RPC failure is not
an input to this method.

SQLite component cases in `repositories/api_wallet/withdraw/self_transfer_tests.rs`:
- `self_transfer_receipt_finishes_success_and_failure_without_nonce_reuse`:
  receipt success/failure records the matching terminal flag, block/time and
  actual fee, clears uncertainty and leaves raw/hash/nonce intact. Pending lookup
  no longer includes the record; backend ACK fields remain empty.
- `self_transfer_receipt_rejects_wrong_identity_and_backend_withdrawal`:
  mismatched hash, chain, trade number and backend trade type cannot be updated.
- `self_transfer_receipt_concurrent_retry_and_opposite_result_cannot_overwrite`:
  one concurrent caller writes; duplicate/opposite results preserve terminal facts.
- `self_transfer_receipt_terminal_write_failure_rolls_back_then_recovers`:
  injected finish-write failure rolls back receipt/status/fee changes. Removing
  the failure permits retry without losing signed data or rewinding the nonce.

Validation: `cargo test -p wallet-database --lib self_transfer` (10 passing cases).
This batch supplies persistence only. API query/recovery integration, receipt
observation policy and identical-raw rebroadcast remain subsequent work.


## Background receipt discovery

`self_transfer_scan_cursor_survives_earlier_completion` verifies bounded id-cursor
pages do not skip a later signed record when an earlier row finishes. Zero limit
returns nothing and backend withdrawals are excluded. Database scan caps pages
at 100. Database checkpoint: all 11 self_transfer component cases pass.


## Automatic ordinary EVM receipt observer (API batch B)

Withdrawal actor startup owns one observer handle and stop signals/joins it.
The application observer uses cancellable phase jitter (0–15 seconds initially),
a production interval of 30 seconds and bounded id-cursor pages of 32. Each
observation has a 35-second outer timeout; shutdown cancels the round immediately.
At the receipt-only checkpoint, observation performed no nonce query or broadcast. The rebroadcast batch below adds guarded nonce observation and identical-raw replay; allocation, signing and backend ACK remain excluded.
Only an observed matching hash with status 2/3, valid mined block/time and finite
nonnegative fee is passed to the dedicated SelfWithdraw receipt transaction.
Missing/invalid/RPC-error results preserve the durable pending record.

Integration cases (fake BNB adapter, real SQLite, existing transfer_nonce suite):
- `ordinary_transfer_observer_preserves_uncertainty_until_valid_receipt`: missing,
  RPC error, nonterminal status and hash mismatch remain uncertain; later receipt
  finishes with actual fee. Repeated rounds skip terminal rows and never resend.
- `ordinary_transfer_observer_records_onchain_failure_without_resending`: a mined
  failed receipt sets failure without releasing/reallocating nonce or broadcasting.
- `ordinary_transfer_background_observer_cancels_restarts_and_retries`: cancel a
  hung query, restart observation from saved DB records, see initial missing result,
  then finish on a later timer tick without a manual query. Test interval is 20ms;
  production actor supplies 30s. This tests observer restart, not a full OS process
  restart. Pending raw/hash and nonce remain available across cancellation.

Validation: transfer_nonce integration target with fake RPC; rebroadcast coverage is added below.
App shutdown/suspension stops execution; observation resumes when its actor starts.


## Bounded identical-raw replay (database A, API B)

Scope is API/outbound-wallet manual SelfWithdraw ETH/BNB transfers, despite the
historical `ordinary_transfer` test names. The standard wallet is not covered.
The observer and initial submission share the sender gate; after acquiring it,
the observer reloads the row and skips terminal/error facts. Receipt confirmation
still takes precedence. When no receipt is returned, replay requires saved EVM
bytes matching the saved hash, a non-frozen sender, an absent node transaction,
and pending nonce exactly equal to the saved nonce. RPC errors or unequal nonce
leave the record pending. Missing receipt alone is not proof of a dropped tx.

Before broadcast, a guarded SQLite update claims a persistent attempt with a
60-second cooldown and a lifetime limit of three recovery attempts. Cancellation,
RPC timeout, acceptance and reopening the DB do not refund this budget. Only the
saved bytes are submitted; signing and nonce allocation are not repeated. Node
acceptance does not complete the transaction. At the cap, receipt observation
continues, but no further automatic replay occurs.

SQLite cases:
- `self_transfer_rebroadcast_claim_serializes_cools_down_and_caps_retries`:
  concurrent claims have one winner; fresh/cooling rows and exhausted budget skip.
- `self_transfer_rebroadcast_claim_guards_identity_and_terminal_state`:
  wrong identity/raw, other trade types and terminal records cannot claim.
- `self_transfer_rebroadcast_acceptance_keeps_budget_across_reopen`:
  re-acceptance clears uncertainty without resetting persisted retry count.

Integration cases, fake BNB adapter and real SQLite:
- `ordinary_transfer_rebroadcast_uses_saved_bytes_after_cooldown`: immediate
  observation does not send; after cooldown it sends identical bytes with the
  same hash/nonce and only one initial build; node acceptance is recorded.
- `ordinary_transfer_rebroadcast_waits_on_visibility_nonce_and_freeze`: visible
  tx, higher/lower/unavailable nonce, visibility RPC failure and freeze skip replay.
- `ordinary_transfer_rebroadcast_timeouts_consume_bounded_budget`: timeouts spend
  attempts, preserve signed facts/floor and stop after three recovery attempts.
- `ordinary_transfer_rebroadcast_rejects_corrupted_saved_raw`: hash mismatch
  prevents both claim and send.

Validation commands:
- `cargo test -p wallet-database --lib self_transfer`
- `cargo test -p wallet-api --test mod --features integration-tests transfer_nonce`

No live ETH/BNB RPC, full mobile process restart or production recovery is claimed.
The budget counts recovery attempts; the existing transport's authentication retry
may make more than one network call within an attempt.


## Async future compiler depth

Library and integration test crate roots set `recursion_limit = "256"` so the
compiler can resolve `Send` for nested transfer and observer futures. This changes
compiler analysis only, not runtime retry counts, timeouts or stack limits.
Validation uses `cargo test -p wallet-api --test mod --features integration-tests
ordinary_transfer_` and checks its build output for absence of
`recursion_depth_exceeding_limit` / `overflow evaluating the requirement`.
The existing cases cover durable submission, receipt observation, cancellation
and bounded replay with fake chain calls and real SQLite.

Replay fixtures bind nonce responses to their own sender. Unknown senders fail
observation rather than inheriting the active scenario's nonce, since the shared
SQLite scan can include earlier scenarios' still-pending signed transactions.

## Cancellation after a durable replay claim

`ordinary_transfer_rebroadcast_cancellation_preserves_claim_on_restart` hangs the
fake broadcast after its persistent claim and send entry. Shutdown cancels the
observer promptly. A recreated observer and complete observation round preserve
uncertainty and the spent attempt without sending during cooldown. Once aged,
the same raw/hash/nonce is accepted, with one initial build and two spent recovery
attempts (three sends including initial submission). Assertions read real SQLite
and fake send bytes; this models task restart, not full mobile process death.

Validation: `cargo test -p wallet-api --test mod --features integration-tests ordinary_transfer_rebroadcast_`.

## MQTT cannot resolve signed EVM SelfWithdraw

Signed ETH/BNB SelfWithdraw execution belongs to receipt observation. Account
change still synchronizes assets, but its legacy status update is skipped for
these rows; `update_tx_status` repeats this exclusion atomically in SQLite, so
arrival order or a stale read cannot overwrite nonce/hash/fee/receipt fields.
Unsigned legacy rows retain their previous account-change path. This is not a
chain-reorg correction mechanism; no MQTT boolean corrects an observed receipt.

Database regressions:
- `self_transfer_account_change_cannot_override_receipt_or_finish_pending`:
  ETH/BNB message-shaped writes before, concurrent with, and after failure receipt
  update zero rows; signed identity, nonce and actual receipt fee/block survive.
- `self_transfer_unsigned_legacy_account_change_remains_supported`: legacy
  unsigned status update remains supported.

The shared list/detail projection prefers signed EVM chain failure/success facts
and recognizes the observer's Success/Failure statuses. It therefore also reports
failure for historical rows with failure facts plus MQTT narrative status 9.
Existing observer success/failure tests now assert the public bill-detail status;
`ordinary_transfer_observer_failure_survives_legacy_success_projection` explicitly
reproduces the old overwrite and asserts status 3 through bill_detail.

Validation: database self_transfer; API integration ordinary_transfer_observer_.
No live account balances or simulator database are changed by the repair.
Checkpoint results: 16 database cases, 3 observer/detail integration cases, and
3 existing acct_change asset synchronization cases passed; formatting/diff clean.

## Reused manual signed nonce candidates (2026-10-08)

Reuses historical `75333dcb4` repository transaction and `5b6f1a623` manual
observation/application wiring on baseline `462dfa44`; preserves current eight-EVM
classification from `23c6d5b3`. No owner schema, indexes, cleanup/reset API, Core
changes or queued-flow strategy changes.

Manual flow: existing sender gate -> read chain next/local floor without mutation
-> build -> writer transaction recheck floor + immutable raw/hash insert + nonce
floor -> successful commit -> broadcast. Stale/foreign candidates fail without
saving or sending. The original persist method retains its idempotent contract.

- `candidate_nonce_all_evm_build_failure_can_retry_without_gap` and
  `candidate_nonce_all_evm_persistence_failure_can_retry_without_gap`: all eight
  EVM chains leave original floor0/no row/no send on injected pre-send error;
  a later success uses the same observed nonce1 and persists signed facts first.
- `candidate_nonce_cancelled_build_remains_available`: controlled blocked builder
  cancellation leaves floor0, no signed row/send; subsequent call succeeds.
- `candidate_nonce_foreign_allocation_during_build_never_sends`: foreign allocator
  consumes nonce1 while manual builder is blocked; write-side candidate recheck
  rejects manual signed insert, preserves foreign floor1, and sends nothing.
- Database `self_transfer_candidate_*` tests reuse historical commit, insert
  rollback, stale candidate and competing writer cases. Added eight-chain nonce0
  validates signed raw/uncertainty and nonce are committed together. Existing
  `self_transfer_*` tests protect identity, immutable bytes, restart, receipts and
  original persist idempotency.

Validation:
`cargo test --offline -p wallet-database --lib self_transfer` and
`cargo test --offline -p wallet-api --test mod --features integration-tests transfer_nonce`.
Real temporary SQLite and fake chain only; no live wallets/backend/publicRPC.

Cancellation/crash before persistence creates no reservation. During transaction,
rollback leaves neither new signed facts nor a new floor; actual commit leaves
both. Commit driver ambiguity is assessed from this transaction boundary, not
injected at the driver. Application cannot send on a failed persistence return;
existing signed observer can recover committed facts. Signed/uncertain sends and
historical bare reservations are never automatically released. Individual process
kill/serialization/pool errors are not injected. Existing nonce exact-set API and
queued observation helper are unchanged; their old false-only reject_gap branches
are left outside this minimal reuse rather than mixing optional cleanup.

## Ordinary EVM matching submission acknowledgment (2026-10-09)

Supersedes the historical batch-3 visibility requirement for the ordinary signed
EVM initial-send entrypoint only. After pre-send atomic reservation commits,
a real adapter broadcast response matching the saved hash is node submission
acceptance, even if an immediate read would fail or return null. The existing
TransactionResult hash shape is unchanged; it is not a successful chain receipt.
Acceptance is persisted before returning; the observer owns final success/failure.

- `ordinary_transfer_avalanche_ack_survives_visibility_tls_error` and
  `ordinary_transfer_avalanche_ack_survives_visibility_error`: configured
  transport/TLS-shaped error and timeout cannot lose acknowledgment; preserve
  saved raw/nonce/hash, persist acceptance without terminal facts. No immediate
  visibility query is necessary after matching acknowledgment.
- `ordinary_transfer_avalanche_ack_survives_not_yet_visible` and
  `ordinary_transfer_invisible_submission_returns_accepted_hash`: return saved
  hash and persist acceptance without claiming confirmed state.
- `ordinary_transfer_avalanche_wrong_hash_with_visibility_error_stays_unaccepted`
  and existing wrong-response-hash case: never replace local hash/acceptance.
- `ordinary_transfer_avalanche_duplicate_without_visibility_stays_uncertain`:
  a synthetic hash from already-known error remains under old visibility guard.
- Existing all-EVM successful receipt test and new observation-error success
  test: observer can finish persisted accepted rows with no resend.
- Existing timeout/network/cancellation, signed/acceptance persistence failure,
  candidate reservation and bounded saved-byte recovery tests remain required.

Only signed ordinary initial sends choose the acknowledgment policy. Public
broadcast_transfer and ordinary recovery retain existing semantics, including
queued EVM/Solana and non-EVM checks. Offline fake adapter + temporary SQLite;
TLS injection is a transport-shaped synthetic error, not a live TLS handshake.
Validation: transfer_nonce integration group plus affected shared broadcast
regressions; no live wallets, RPC, backend, App or plugin release.


## Sui gRPC construction after Core revision update (2026-10-23)

- Boundary: adapter construction only, before fee/transfer RPC and before any
  durable transaction state or broadcast. Existing lifecycle/recovery is retained.
- Coverage: `sui_grpc_tests` in `domain/api_wallet/adapter/sui_tx.rs`.
- Valid endpoint plus ASCII authentication metadata constructs a lazy client
  using the supplied URL without a running node. Invalid endpoint or metadata
  returns an error before a chain operation can start.
- Layer: component; reuse temporary-context fixture, no chain or backend calls.
- Command: `cargo test --offline -p wallet-api --no-default-features --features
  dev --lib sui_grpc_tests`.
- Limits: constructor checks do not assert full gRPC response decoding, live
  authentication, deadline behavior, transaction simulation or broadcast. Core
  mapping tests supply separate offline evidence; live acceptance remains open.

## Receipt-driven asset publication (2026-10-30)

- Locally signed EVM SelfWithdraw: MQTT keeps bookkeeping, asset metadata and the existing account-change event, but defers asset publication while receipt observation or refresh debt remains. Historical/acknowledged terminal rows and other business flows keep their existing behavior.
- The actual receipt and `receipt_refresh_pending=1` commit atomically. `finished_at` remains a chain fact; backend ACK fields are unchanged.
- The observer refreshes only sender native/token and the owned receiver token, using the existing balance pipeline and `API_WALLET_SYNC_ASSETS` contract.
- Query, persistence or frontend send failure retains refresh debt for later observation rounds/restart; network errors have no finite retry cutoff. Duplicate terminal receipts cannot recreate acknowledged debt.
- DB coverage: self_transfer_tests (receipt rollback, success/revert, matching ACK, restart and additive migration).
- API coverage: `receipt_refresh_selects_only_exact_required_assets`, `receipt_refresh_publishes_only_after_terminal_commit_and_retries_closed_frontend`, `receipt_refresh_defers_mqtt_and_survives_balance_query_failure`.
- Delivery is an idempotent invalidation: a crash after send and before clearing debt can cause a repeated notification. No exactly-once transport claim.

- Receipt observation commits only chain facts + refresh debt. A separately cancellable loop reuses the same 30-second observer cadence for asset refresh; balance RPC delay cannot hold the receipt scanning loop. A newly committed receipt may wait until that refresh loop's next round.
- Controlled-timing regression: `receipt_refresh_hung_first_balance_does_not_block_same_sender_next_transfer` hangs the first terminal balance query; a second transfer by the same sender can still sign/send and commit its receipt, then shutdown cancels refresh while its debt remains.
- Partial-query regression: `receipt_refresh_partial_token_query_failure_does_not_publish_native_only` keeps debt and emits no asset event until native and token observations both succeed.
