# API Wallet Import And Bind Assertion Matrix

Scope: `wallet-api` API wallet import, bind, password, and paired deletion flows.

Rules source: `docs/codex/testing.md` and `docs/codex/testing-strategy.md`.

## Current Standard Tests

### WALLET-2981 create/global-password concurrency repair (2026-11-03)

- Entry points: API `create_wallet` and global `set_all_password`, same Context.
  Per-Context credential gate orders before SETUP/protection/DB locks. Initial
  validation/proof repair is protected; the read-only UID lookup runs outside it.
  Before any init/wallet mutation, reacquire and compare the captured device proof
  to current storage; unchanged proof costs no extra KDF. A changed proof is fully
  re-authenticated under the gate; changed passwords yield PasswordIncorrect (3102),
  while concurrent same-password empty-inventory initialization remains valid.
  Final creation writes/initialization and whole global rotation retain the gate.
- Workers own inputs/guards through completion. Cancelling a caller does not cancel
  an already started worker; it may complete internally, rather than release the
  gate while persistence can still finish. Worker failures map to generic internal
  errors without formatting inputs or task panic payloads. Password/phrase/salt copies
  are Zeroizing. Existing partial backend/rotation failures are not made transactional.
- Standalone `credential_rotation` target owns the global Manager fixture. Two-phase
  fake UID barrier is after real validation and before persistence, not a timed sleep.
  Completed Argon2id rotation must reject stale subaccount/withdrawal creations before
  init/keys/binding calls and row insertion; concurrent same-password first requests
  both succeed despite random proof refresh; fresh old-password requests fail before
  HTTP; new-password requests succeed. All stored phrase AND seed bundles authenticate
  under the new password and reject the old one.
- Failed authentication during rotation must release gate without revoking a valid
  pending creation. Bind-info failure preserves its old partial-row behavior and
  releases gate for later rotation. Cancelling the caller while keys-init is gated
  must leave its owned worker alive and its credential gate observed locked before
  any rotation is started. It completes init and final rotation credentials agree;
  the test does not infer a strict init/rotation order solely from their final values.
- No new API/dependency/schema/App changes. Existing proof-reuse tests verify no
  unconditional proof regeneration is reintroduced. Import/reset/delete, ordinary
  creation, multi-Context/process same DB, crash recovery, signing and production
  device/recovery scale are not acceptance claims for this bounded repair.

### WALLET-2981 validated password proof reuse (2026-11-03)

- Only API creation consumes the internal validation outcome. Other callers retain
  `Result<()>`; password verification, protection setup/migration, owned-task and
  cancellation behavior are retained. No KDF parameter changes or authentication cache.
- Validation returns only its authenticated proof snapshot (or a successfully
  persisted authenticated fallback proof). Creation checks this snapshot against
  the policy after protection preparation: current KDF algorithm, factory default
  parameters and generator salt length must match. Otherwise retain one rebuild;
  next creation can reuse it. Ordinary validation adds no policy-read failure path.
  Reuse does not write proof contents back.
  First wallet, legacy migration and token shortcuts retain generation; a stored
  but unverified proof is never sufficient. Existing create-vs-password-rotation
  lifecycle serialization is not redesigned; skipping overwrite avoids restoring
  stale proof contents but does not make the whole creation transaction atomic.
- Standalone integration target `creation_proof_reuse` uses the existing offline
  Manager/SQLite/fake backend in its own process (fresh inventory is required).
  It explicitly sets modern Argon2id: `new_for_test` skips App initialization and
  otherwise falls back to Scrypt. Performance uses the identical fixture/profile/
  dependency lock in one temporary package path, swapping only four before/after
  production files; early default-Scrypt timings are not a modern-policy baseline.
  Logs and source provenance live in `target/wallet-2981-proof-reuse/argon-*`
  and `baseline-source.json`; uncommitted source is reviewed by fixed fingerprints.
  It audits real proof write counts without recording contents: first=1,
  subsequent subaccount/withdrawal=0 and proof unchanged, missing=fallback 1;
  wrong password/corrupt proof/persistence failure abort before HTTP/proof writes.
  Success checks UID, wallet type, initialized state and backend order. A real old
  Scrypt proof under upgraded Argon2id configuration is rebuilt once, then reused;
  pure policy tests cover algorithm/parameters/salt size for all three KDFs.
- `proof_reuse_status_*` library tests cover unverified proof in empty inventory,
  token outcome and V1 authentication/migration. Existing protection-password-entry
  tests guard shared-validator behavior. Host timing is an offline/debug comparison,
  not a promise of iOS/Android/device latency; original minute-long failure remains
  unreproduced. The pre-existing console-worker feature-gating build failure is
  outside this change; an emitted exact integration executable can be run directly.

### WALLET-2981 creation diagnostics

- Scope: `ApiWalletService::create_wallet` only; no import/recovery, schema,
  authentication, cryptographic parameters, backend ordering or task semantics
  change. Process-local call IDs distinguish overlapping calls on the same device.
- INFO `metric="api_wallet_create_stage_ms"`: `event=begin/end`, `call_id`,
  `wallet_type`, `stage`; end adds `elapsed_ms` and `outcome=ok/error`.
  `cancelled_or_unwound` means the observed future was dropped/unwound, not that
  other owned work was cancelled. No raw inputs, outputs or error details.
- Read `stage=total` for Rust service duration; it excludes App/FFI dispatch and
  subsequent UI refresh. Nested stages overlap and must not be summed as peers.
  `http_*` includes backend preparation, queueing, retry and response parsing,
  not pure wire latency. `wallet_upsert` includes ciphertext generation and DB.
- Reuse `writer_gate_wait_ms` to identify application writer-gate waits (INFO
  from 100ms, WARN from 1000ms); `write_tx_duration_ms` includes the operation and
  SQLite busy retries after that gate. Match device/time/stage and `op` (e.g.
  `upsert_api_wallet`); existing DB metrics do not carry the creation call ID,
  so concurrent same-op calls cannot be attributed exactly from these alone.
- Offline timing tests exercise returned success/error identity, INFO visibility,
  safe fields, overlapping-call correlation and nested cancellation completion.
- Device acceptance remains: same package/build, count actual address rows, record
  recovery state, then compare small / 500k idle / 500k recovering. Capture total,
  expensive stage, gate waits and write duration. Missing logs or a synthetic
  fixture do not establish 500k-device readiness. No real signing/broadcast.

2026-11-02 offline synthetic experiment (explicitly selected by user; repeated on
GitLab develop base `26c0fc66` with this diagnostic patch before publication):

- `creation_scale::wallet_creation_database_scale_probe` calls the real Manager,
  reuses the existing fake backend, and creates both wallet types with backend
  `bind_status=false`, locally linking withdrawal to its freshly created subaccount.
  Existing synthetic addresses live in an unrelated background wallet; assets,
  recovery workers, chain RPC, MQTT and real HTTP are not reproduced. Fresh DB
  applies current migrations; results do not establish old-package/index parity.
- After warm-up, 3 samples per type/case (milliseconds, medians):

  | Background address rows | Subaccount wallet | Withdrawal wallet |
  | --- | ---: | ---: |
  | 100, idle | 2161.454 | 2167.502 |
  | 500,000, idle | 2167.447 | 2169.285 |
  | 500,000, controlled writer | 2366.267 | 2367.343 |

- No slowdown due solely to address count was reproduced in this experiment.
  These are host/debug, warm-database samples, not simulator timings or a speedup
  claim. Idle 500k's largest stage is UID PBKDF2 (~902–905ms), then wallet_upsert
  (~518–520ms) and unlock_material (~518–520ms); this does not identify the QA root cause.
- Controlled case writes at most 1000 existing rows through a competing real
  transaction/application gate, then intentionally holds it 200ms while creation
  enters `db_mark_init`. Observed existing gate metric 201.622–203.440ms; this
  validates attribution, not the actual duration/behavior of address recovery.
  The fake keys-init barrier also includes writer setup in `http_keys_init`.
- Probe passes (77.03s), checks successful UID/type/init/binding, backend call
  ordering, exact background count/identities, and bind-info failure's total/error
  event plus unchanged pre-existing partial persistence. Logs/results are under
  `target/wallet-2981-publish/sdk-scale-run.log` and `target/wallet-2981-publish/sdk-scale-summary.json`.
- Test executable `2389778aa7f2a403` was emitted fresh by Cargo JSON and executed
  standalone with isolated process TMPDIR. Aggregate integration build exits101
  on the pre-existing console-worker feature gating error; it is NOT a passing
  whole-build result. This task leaves that unrelated binary unchanged.


### Expand Planning Crash Boundary

- Integration `expand_planning/` uses real SQLite and the production planner:
  recover a persisted Running batch with zero items, retry second-chunk SQL
  failure after reopening the DB, and verify disjoint indices for concurrent
  same-UID/chain batches. New Planner instances read durable facts; no process
  kill or real backend/chain operation is performed.
- Atomicity: repository-owned write claim, existing read-only index calculation,
  and all 150-row insert chunks share one transaction. Failure/cancellation
  rolls back the claim and every chunk; the existing writer lock serializes
  local planners and the SQLite write transaction freezes other writers.
- Address-query gate is rechecked at the write claim. Running empty batches can
  recover; any existing item prevents reallocation of a frozen batch.
- DB tests `expand_planning_*` cover rollback/retry, controlled cancellation and
  query gate recheck, including legacy Running-empty recovery.
- Scope remains the existing UID+chain index queries, no derivation, HTTP or
  protocol changes. Existing Running batches with partial nonempty legacy item
  sets are preserved, not automatically repaired by this change.
- QA mapping TC-APP-010 / TC-EXP-003/005; supplied QA XMind stays unchanged.


### Expand Bootstrap Offline Recovery

- Entrypoint: `tests/integration/api_wallet/expand_bootstrap/`, production first
  unlock bootstrap plus real scanner/Notify worker, temporary SQLite and shared
  loopback HTTP backend. Each scenario runs in a child process to isolate static
  scanner/worker state. Run with `dev,integration-tests` to use the local fixture.
- A connection dropped after receiving completion must not block scanner startup.
  The existing scanner retries the unnotified Done batch, and the worker writes
  `expand_complete_at`; the scanner then advances it to Notified.
- Concurrent/repeated unlock preserves the same event channel; a successful
  completion is not duplicated by a separate synchronous bootstrap reporter.
- External assertions check retry count and UID/batch/serial payload; DB checks
  require both local and remote completion facts with terminal Notified state.
- Actual RED: both scenarios fail on old code (startup transport error and two
  successful completion requests instead of one). No new queue, protocol or
  account derivation path is introduced. Provided QA XMind is unchanged.
- Scope: TC-EXP-005 / TC-APP-009/010 startup recovery; not the separate
  Pending-to-Running item-creation crash window or an address-scale benchmark.


### Paired Wallet Deletion And Cleanup Retry

- Entrypoint: `WalletManager::physical_delete_api_wallet`; integration cases in
  `tests/integration/api_wallet/wallet_deletion/` use the shared fake backend and
  real temporary SQLite. The supplied QA XMind is not modified.
- SubAccount deletion removes the reciprocal pair and its accounts, while
  unrelated wallets/accounts survive. App has no delete-Withdrawal entrypoint
  (user confirmation, 2026-10-16); reverse cascading is not a requirement.
  Inconsistent bindings are
  rejected before backend calls or local deletion.
- Backend failure retains the pair/accounts for retry. After recovery the same
  public call retries backend cleanup; repeated/concurrent same-SubAccount successful
  deletion does not repeat that cleanup.
- A retry after local deletion already committed still finishes device/session
  bookkeeping; an injected device UID write error must not leave a permanently
  stale UID merely because the wallet is now absent.
- Existing address-unbind tasks are durably stored before local deletion,
  including inactive addresses. A task-write failure leaves local retry facts;
  tests check the stored payload independently of dispatch, not merely a return.
- Controlled backend gating adds an address after the snapshot. The transaction
  compares the current ordered id/chain/address fingerprint against that snapshot and
  rolls back on mismatch; the new address survives and a fresh retry durably
  includes it. This does not assert global serialization of all wallet writes.
- Local atomicity: `wallet-database` repository tests
  `api_wallet_deletion_snapshots_inactive_and_deletes_only_selected_data` and
  `api_wallet_deletion_local_failure_rolls_back_wallets_accounts_and_query_state`
  check selected scope, empty selection, and rollback after injected SQL failure.
- WALLET-2980 records PRAGMA data_version on its retained physical reader BEFORE
  the first BEGIN/table read. The borrowed read transaction fixes a pre-backend
  view through the real account-watermark query; wallet/UID/page reads share it.
  After backend success, one fixed-upper-bound1000-row page walk persists bounded
  cleanup and computes SHA-256. Explicit read rollback ends the WAL snapshot but
  retains the original connection/token. Writer BEGIN IMMEDIATE precedes the
  same-reader token recheck: unchanged skips ALL second identity scans; changed
  (even unrelated commits) runs the existing in-write scan against the original
  cleanup fingerprint. No unconditional fresh second-reader scan remains.
  A lost first-attempt source (e.g. SQLite busy error) forces full write-side
  validation on retry; never recapture a token and reuse old cleanup facts.
  Legacy no-source and shared/clone read-write pools use the safe fallback;
  shared source is dropped before writer acquire to free the sole slot. SQL
  deletes without RETURNING *, guards/counts/query-state cleanup remain one atomic
  transaction; no partial-commit deletion, schema revision or raw transaction Drop.
- `wallet_deletion_cleanup_requests_are_bounded_and_durable` checks count bounds,
  persisted payload coverage and actual deletion with 2,504 selected addresses;
  `wallet_deletion_cleanup_requests_bound_escaped_payload_bytes` checks the
  128 KiB serialized-request bound, including JSON escaping.
- Cleanup writes reuse the existing TaskQueueRepo backend-task builder and
  persistence, one bounded request per write, without intermediate JSON maps.
  Consumer wire-schema compatibility is asserted with escaped/UTF-8 fields.
  `wallet_deletion_partial_cleanup_persistence_failure_is_retryable` uses 20,504
  addresses and fails after the first 16 persisted tasks; it reopens recovery,
  checks all local facts survive, and retries with complete durable coverage.
  `wallet_deletion_single_oversized_address_keeps_retry_facts` rejects an
  individual entry exceeding the request cap, preserving local wallet/accounts.
- `wallet_deletion_500000_addresses_have_bounded_durable_cleanup` is an explicit
  ignored scale test: 500,000 unique addresses (mixed active/inactive), the public
  deletion entrypoint, real SQLite and fake device-delete backend; verify each
  persisted task's item/byte bound and unique address coverage using disk-backed
  test SQL, plus unrelated wallet preservation. Execution evidence: PLANS.md and
  target/wallet-2980 and target/wallet-2980-perf logs. No real backend request
  or cleanup worker execution.
- Latency follow-up: reuse already-locked ring SHA-256 for the ephemeral
  fingerprint; the independent identity-encoding golden test covers byte-length
  prefixes, UTF-8, pagination independence and equal-concatenation counterexample.
  Fixed-bound pages transfer as one bounded SQLite JSON aggregate, avoiding
  per-row SQLx transport; escaped/UTF-8 and integer IDs above 2^53 roundtrip exactly.
  Existing DELETE/query-state cleanup/commit remains intact. The immutable DB
  snapshot replaces the initial full fingerprint scan; final comparison uses a
  fresh upper bound and detects late rows/changed identities.
  Stage metrics target `wallet_deletion_perf` reports counts/durations only.
  Explicit scale test enables just this target. Comparative experiments and
  final500k measurements are recorded under target/wallet-2980-perf,
  target/wallet-2980-snapshot, target/wallet-2980-lock,
  target/wallet-2980-one-pass and PLANS.md.
- Stable-read regression: one-reader/one-writer pool tests prove old identities
  stay visible during committed concurrent updates, explicit close precedes a
  fresh writer, drop returns reader and enables checkpoint/truncate.64x64KiB
  updates proceed while snapshot held; WAL1071frames remain pinned until release.
- Backend transport Timeout, caller deadline and task abort tests hold the
  backend on barriers, apply same WAL pressure, assert release/checkpoint recovery,
  preserve wallets/accounts/no cleanup and then retry successfully. Transport's
  existing30s budget is unchanged; no real HTTP timing claim.
- Runtime concurrent-account/rebind tests initialize fixture pools/replacement
  before starting the backend window; actual INSERT/UPDATEs remain inside it.
  New SqliteContext constructors run existing startup TRUNCATE and are not runtime
  writes. Their possible busy-timeout wait is not changed by this repair.
- Writer-window regression tests cover identity/new-ID/binding changes between
  validation and lock, including a separate SQLite pool, unrelated commits that
  permit fallback completion, DELETE failure rollback/retry, cancelled pending
  finalizer returning the sole reader, and legacy shared-single-slot completion.
  `wallet_deletion_500000_addresses_read_write_waits` is a standalone ignored
  real-SQLite/fake-backend experiment: stage events trigger reads, writes through
  the SAME application pool/gate and an independent writer. First-snapshot-phase
  committed writes intentionally cause conservative fallback; both stages report elapsed
  waits and assert readers progress. Concurrent probe run is separate from the
  uncontended500k benchmark. `writer_occupied` includes post-BEGIN IMMEDIATE
  writer-connection/gate occupancy through commit (including auto-checkpoint).
  Contended fallback retains a longer writer window; atomic DELETE itself still
  blocks other writers. No claim that background execution removes DB contention.
- First-source tests capture a commit BETWEEN token/first table read, change
  identities DURING the first view, reject stale facts after SQLite busy retry,
  and prove baseline is never refreshed. The busy-trigger controller waits for
  the consumed sole-reader slot, then drops the trigger/changes identity through
  the writer slot before retry; original facts survive, a fresh source succeeds.
  Retained idle source is separately checked: closing its borrowed read allows
  WAL TRUNCATE before source Drop; Drop returns the single reader. A cloned
  shared single-slot source also completes without self-wait. Existing API
  backend timeout/deadline/abort tests cover the actual borrowed-read ownership.
- WAL pinning during the read lifetime is expected, with no hard byte cap. Tests
  explicitly checkpoint/truncate after release; production does not truncate
  automatically on each deletion. Real forced-kill/mobile/release remain untested.
- Limits: no live backend/real-device deletion or forced process-kill recovery.
  Peak RSS includes test fixture/SQLite setup and verification; it does not prove
  mobile memory limits. Existing dispatcher/recovery can queue multiple request
  bodies; this change bounds deletion preparation and each request, and retains
  task IDs for dispatch rather than holding every body in the deletion future.
  Remote deletion and the local transaction
  are separate operations; local retry facts and durable unbind authority cover
  their failure boundary, not a distributed atomic transaction.

### Import Subaccount Wallet

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/import_subaccount.rs
  import_subaccount_wallet_ok_unbound
  import_subaccount_wallet_sets_progress_stage_before_completion
  ```

- Backend: fake API-wallet backend reports `ApiRaw` and unbound UID info.
- DB facts: subaccount wallet is persisted with `sn`, empty app/merchant
  fields, and final `import_stage = 3`.
- Invariant: a successful subaccount import must finish local import state and
  call UID check, bind-info query, init, and old-keys init paths.

### Import Subaccount Rejection

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/import_subaccount.rs
  import_subaccount_wallet_query_failure_does_not_persist_half_state
  import_subaccount_wallet_uid_status_mismatch_rejected_without_persist
  ```

- Backend: fake backend either fails bind-info query or reports `ApiWaw`.
- DB facts: expected UID does not exist after rejection.
- Invariant: rejected subaccount import must not persist half-state or continue
  into key initialization after preflight failure.

### Import Withdrawal Wallet

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/import_withdrawal.rs
  import_withdrawal_wallet_ok_requires_binding_address
  import_withdrawal_wallet_recovers_incomplete_subaccount_then_completes
  import_withdrawal_wallet_reimport_keeps_completion_and_account_count_stable
  import_withdrawal_wallet_with_concurrent_asset_reads_succeeds
  ```

- Backend: fake backend reports `ApiWaw`, bind-info, and app-id usage facts.
- DB facts: withdrawal and recharge wallets bind to each other, share app and
  merchant ids, and finish `import_stage = 3`.
- Invariant: withdrawal import must bind to an existing recharge wallet, recover
  incomplete local state, tolerate reimport, and remain readable while import is
  in progress.

### Import Withdrawal Rejection

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/import_withdrawal.rs
  import_withdrawal_wallet_uid_usage_false_rejected_without_persist
  ```

- Backend: fake backend reports `ApiWaw`, bind-info, and app-id usage `false`.
- DB facts: expected withdrawal UID does not exist after rejection.
- Invariant: withdrawal import must not persist a wallet or initialize keys when
  app-id usage validation fails.

### Foreground Import Recovery Priority

- Layer: unit.
- Entrypoint:

  ```text
  infrastructure::recovery::recovery_coordinator::recovery_coordinator_tests
  ```

- Runtime facts: a valid API-wallet import registers its UID before foreground
  persistence; address-list pages and deferred recovery tasks wait before doing
  unrelated recovery work.
- Invariant: foreground recharge and withdrawal UIDs proceed immediately,
  unrelated recovery yields without holding a DB transaction, and an abandoned
  foreground lease expires instead of blocking recovery permanently. A global
  pause has precedence over foreground-wallet admission.

### Ephemeral Recovery Task Retention

- Layer: SQLite repository regression plus task-manager completion policy unit coverage.
- Entrypoint:

  ```text
  wallet-database::dao::task_queue::tests::delete_done_by_task_name_preserves_other_names_and_statuses
  task_manager::tests::only_deferred_api_account_recovery_is_deleted_after_success
  ```

- DB facts: only Done `CreateApiAccountDeferred` rows are deleted; matching Pending/Failed rows
  and unrelated Done MQTT rows remain.
- Invariant: successful large recovery payloads are ephemeral, while unfinished retry payloads and
  MQTT/JPush deduplication records remain durable.

### Bind Relation

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/bind_relation.rs
  scan_bind_ok_calls_backend_and_persists_bind_sn_and_relation
  import_bind_ok_calls_appid_import_and_persists_bind_sn_and_relation
  ```

- Backend: fake backend records one scan bind or app-id import call.
- DB facts: both wallets persist `sn`, app id, merchant id, and reciprocal
  binding addresses.
- Invariant: successful bind APIs must persist the same relation fields on both
  wallet records.

### Bind Relation Rejection

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/bind_relation.rs
  import_bind_backend_fail_does_not_persist_relation
  scan_bind_backend_fail_does_not_persist_bind
  import_bind_missing_wallet_returns_not_found_and_no_backend_call
  scan_bind_remote_first_then_persist
  ```

- Backend: fake backend can fail scan bind or app-id import.
- DB facts: existing bind fields remain unchanged on backend failure or missing
  wallet.
- Invariant: remote bind/import must happen before local relation persistence,
  and missing wallet must not trigger backend import.

### API Wallet Password Refresh

- Layer: integration.
- Entrypoint:

  ```text
  tests/integration/api_wallet/import_bind/password.rs
  change_password_refreshes_api_wallet_unlock_session
  ```

- Backend: fake backend reports `ApiWaw`.
- DB facts: withdrawal wallet imports successfully before password rotation.
- Invariant: password rotation must refresh the API-wallet unlock session so a
  later sync can still run.

## Template Contract

`import_bind/mod.rs` is a directory-level organizer:

1. Keep import, bind relation, and password tests in flow-specific files.
2. Keep only shared import phrases and password constants in
   `import_bind/support.rs`.
3. Keep direct harness and fake-backend setup inside the owning subflow file
   until a subflow needs deeper scenario extraction.
4. Preserve existing test names so targeted filters remain stable.

`import_bind/password/mod.rs` uses a password-refresh integration contract:

1. Keep password-refresh test cases read-first in `password/mod.rs`.
2. Use `PasswordRotationScenario` with shared `ScenarioRoles`.
3. Keep password-flow business methods in flow-local
   `PasswordRotationGiven`, `PasswordRotationWhen`, and
   `PasswordRotationThen` traits.
4. Use `scenario.given()` for fake backend UID status.
5. Use `scenario.when()` for wallet import, password rotation, timer tick,
   chain-data sync, and password restoration.
6. Use `scenario.then()` for persisted wallet type assertions.
7. Keep harness environment reset, manager calls, backend queue setup, waits,
   and DB loads below the test body in `password/support/scenario.rs`.

`import_bind/import_subaccount/mod.rs` uses the role-trait Given-When-Then
shape:

1. Keep subaccount import cases read-first in `import_subaccount/mod.rs`.
2. Keep fake backend environment in `SubaccountImportScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep backend UID status and bind-info setup in the flow-local
   `SubaccountImportGiven` trait.
5. Keep import success and failure entrypoints in the flow-local
   `SubaccountImportWhen` trait.
6. Keep wallet DB facts, persisted import stage, error facts, and backend-call
   assertions in the flow-local `SubaccountImportThen` trait.
7. Keep manager calls, DB loads, and load/assert internals below the
   Given-When-Then test body.

`import_bind/import_withdrawal/mod.rs` uses the role-trait Given-When-Then
shape:

1. Keep withdrawal import cases read-first in `import_withdrawal/mod.rs`.
2. Keep fake backend environment in `WithdrawalImportScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep backend UID status, bind-info, app-id usage setup, slow-backend guard,
   and recharge wallet seeding in the flow-local `WithdrawalImportGiven` trait.
5. Keep import success/failure entrypoints and concurrent asset reads in the
   flow-local `WithdrawalImportWhen` trait.
6. Keep wallet binding facts, completion facts, reimport stability, concurrent
   read result, rejection facts, and backend-call assertions in the flow-local
   `WithdrawalImportThen` trait.
7. Keep manager calls, DB loads, fake backend queues, and concurrency helpers
   below the Given-When-Then test body.

`import_bind/bind_relation/mod.rs` uses the role-trait Given-When-Then shape:

1. Keep bind relation cases read-first in `bind_relation/mod.rs`.
2. Keep fake backend environment in `BindRelationScenario`.
3. Expose `scenario.given()`, `scenario.when()`, and `scenario.then()` as the
   fixed read-first integration roles using generic containers from
   `tests/harness`.
4. Keep backend failure setup, wallet pair seeding, single-wallet seeding, and
   bind snapshots in the flow-local `BindRelationGiven` trait.
5. Keep scan bind, import bind, and missing-wallet entrypoints in the
   flow-local `BindRelationWhen` trait.
6. Keep persisted bind fields, unchanged snapshots, error facts, and
   exact backend-call assertions in the flow-local `BindRelationThen` trait.
7. Keep manager calls, DB loads, snapshot helpers, and backend recorder checks
   below the Given-When-Then test body.

`tests/harness` remains reserved for cross-flow environment and fake
capabilities. `src/testkit` remains reserved for crate-private worker or
scanner entrypoints.

## Gaps To Close Next

- Extract scenario helpers for the withdrawal import flow if more import
  recovery tests are added.
- Add explicit bind relation concurrency coverage if the local persistence
  order changes.

2026-10-20 delivery review: deletion also fences wallet id/uid/type/binding snapshots in the same local transaction; changed relations roll back, preserving selected wallets/accounts for retry. Public change-withdrawal serializes with existing deletion lock before backend/read work. New wallet_deletion controlled backend-gate tests cover stale binding and public rebinding serialization; database identity rollback covers changed id/uid/type/binding. No distributed rollback claim for already completed backend operations or external writers bypassing public services.

2026-11-02 normal-scale characterization: ignored `wallet_deletion_normal_scale_timing` uses synthetic 100/1000/10000 address fixtures, three fresh-process repeats for plain deletion, unchanged-data fast path, and first-snapshot concurrent writes forcing fallback (27 passing samples). Final reservation probes issue real reads and same-app/external writes; timestamps reject probes starting after writer occupancy ends. Plain deletion ranges: 4.531–7.211 / 21.801–23.056 / 198.787–203.588 ms. Fast-path same-app write waits: 1.492–1.785 / 9.261–9.614 / 93.891–102.468 ms; fallback: 1.960–2.188 / 13.162–14.166 / 123.363–125.475 ms. All sampled reads below 0.4 ms; external write maximum 128.261 ms. These measure operation latency including scheduling, pool/gate and SQLite contention, not isolated lock duration. Real SQLite on this host with fake backend/Noop cleanup execution; excludes network, UI and physical-device performance. Evidence: `target/wallet-2980-normal-scale/{samples,summary}.json` and per-case logs. No production code changes in this round.

2026-11-02 structural cleanup (no behavior change): stable deletion source/read/fingerprint and guarded finish live in `wallet-database/src/repositories/api_wallet/wallet/deletion.rs`, with existing public type paths re-exported by `wallet.rs`. Deletion component regressions are in `wallet/deletion_tests.rs`; only-test snapshot/page/read construction lives there as private test-only methods. Ordinary wallet component tests and shared seed helper live in `wallet/wallet_tests.rs` and `wallet/test_support.rs`. API flow support is split into `support/{scale,wal,concurrency,performance,normal_scale}.rs` and the core scenario in `support.rs`. Transaction statements, source token timing, retry fallback and cleanup payload logic are preserved. Validation records: `target/wallet-2980-cleanup/`.

2026-11-02 mixed-scale characterization: ignored `wallet_deletion_mixed_scale_timing` reuses normal-scale timing/probes and bounded cleanup coverage, adding a separate wallet with500000synthetic active+inactive addresses. Background rows precede the target's bulk addresses, leaving a large global-id gap after the4initial target rows. Fixture construction and background verification occur outside the measured service call. Count/min/max/sum-id and expected address/uid/chain/status predicates agree before/after; background wallet and backend binding UID survive; cleanup verifier rejects unrelated identities. Existing single-address probe wallet is kept unchanged. Same-binary small-database controls, alternating order and fresh-process fixtures distinguish total deletion from concurrent operation latencies. No production query or deletion behavior changed. Logs and final statistics under `target/wallet-2980-mixed-scale/`; host-only warm-cache-friendly synthetic evidence, not UI/network/real-device acceptance.

Mixed-scale results:54base samples (3repeats*3target sizes*3modes*2datasets) plus4targeted rechecks pass correctness/coverage checks. Plain deletion median control→mixed:100=4.167→106.983ms,1000=21.058→144.103ms,10000=195.274→312.083ms. Across all measured modes/rechecks mixed worst deletion6.619/4.218/3.242s, competing final write maximum3.448/2.126/2.414s; point reads<=0.647ms. Slow reads retained, no outlier removal;100changed recheck repeats3.150s first read and3.385s write-held fallback. Performance is materially degraded in this layout; no no-blocking claim. EXPLAIN shows global rowid range filtering, not wallet-index paging. Host/cache/I/O effects on tail magnitude remain unisolated. `comparison.json` keeps three-repeat plain medians separate from targeted rechecks; all raw logs retained.

2026-11-02 wallet-scoped read optimization: migration `20261102000000_api_account_wallet_id.sql` adds(wallet_address,id). Deletion DAO seeks each unique selected wallet through the same mutable transaction connection, takes<=1000candidates per wallet and merges/truncates by globalid after each wallet; original cursor/cutoff/fingerprint ordering remains. No new proof/state/worker. VM-step regression with3targets and10000unrelated rows fails old query90110steps, new106steps; duplicate wallet keys, inactive targets after foreign gap and cutoff exclusion checked. Populated-data migration test verifies index columns and unchanged identities. DB33tests pass; source/read/token/retry/service files are unchanged. API18failure/cancellation/WAL/concurrency regressions pass. Performance evidence under `target/wallet-2980-wallet-read/`; old and optimized binaries share identical measurement code, with index absence/presence checked in every fixture. First-time index build cost is outside deletion measurements; existing-data correctness covered, large upgrade duration not benchmarked.

Wallet-scoped read results: mixed old/new binaries,3repeats per100/1000/10000 perplain/fast/changed (54cases), all background500000addresses preserved. Plain median old→new(ms):114.410→22.759 /147.782→44.260 /325.340→251.959. Changed-source final write ranges old→new(ms):105.558–125.471→5.376–11.035 /112.162–131.303→17.321–49.107 /244.400–394.810→149.452–177.803. Fast-path writes remain similar, some new maxima higher; index maintenance retained. New mixed deletion ranges across modes:6.717–28.739 /44.203–258.952 /240.450–283.471ms, sampled point reads<=3.950ms. No seconds-long mixed samples this round; no universal latency guarantee.
Target500k plain/fast/changed old/new plus another fast pair (8cases) all500000unique identities/500tasks. Plain old/new13.890/14.247s; changed15.674/14.344s; fast control13.637/13.371s vs new15.820/15.657s (16–17%slower in these two pairs), pooled writer fast control8.364/7.585s vs new8.749/8.772s. This is an explicit remaining performance cost, not no-regression or full-scale speedup evidence. Saved all samples; comparison.json/target500k-comparison.json/validation.md. Single host, index build during upgrade excluded, extra index and read roundtrips may add costs; no precise attribution or tail probability estimate.

2026-11-02 fast-path follow-up candidate (uncommitted): keep the wallet-scoped paging/query, retained source/data_version timing, BEGIN IMMEDIATE validation, first-attempt proof consumption, retry rescans and atomic guards unchanged. Forward migration `20261102010000_api_account_wallet_rowid.sql` narrows the existing index to(wallet_address): INTEGER PRIMARY KEY id is the implicit rowid already appended by SQLite, so explicit id was duplicate storage. Preserve the previous migration checksum. Existing-data migration now checks identities and rowid range seek without a temp sort; mixed work remains106VM steps. Cleanup reuses TaskDbPool and existing create_task without RETURNING payload, collects its generated ID only after successful persistence; no task batching/granularity change. Component3 (same helper source), DB33, supported API build and API18/4ignored pass; API coverage includes timeout/cancel/WAL release/partial-failure recovery/concurrent identities and binding. CTE single-statement and TEMP-ID prototypes were slower and rejected, not in delivered code.

Final same-source performance matrix:66cases plus8separate targeted mixed rechecks, all correctness/coverage/background/probe-overlap assertions pass. Fresh process/fixture percase, sequential version-order alternation, actual index columns checked. Target500k fast3-repetition median control→candidate13.247→10.806s; same-app write wait8.221→5.973s median, candidate range5.904–7.492s; candidate final-window point reads0.108–0.464ms. Plain single sample13.418→10.722s; changed single14.586→14.617s (no measured benefit). Mixed plain3-repetition medians100/1000/10000 addresses21.730→6.789 /41.898→40.019 /219.467→224.706ms. Mixed fast medians23.952→234.800 /29.161→42.910 /211.462→390.235ms: explicit regression in this matrix, not all-scale/no-regression acceptance. Separate8targeted rechecks:100fast control24.830/6.050 vs candidate13.382/24.690ms;10000fast control230.309/228.061 vs candidate232.506/228.104ms. Rechecks do not replace original66or prove absence of code/build/scheduling/I/O effects. Candidate mixed final-window write maxima across fast/changed29.149/18.430/145.014ms for100/1000/10000; point reads<=3.187ms. These include scheduling/pool/gate/SQLite waits, not pure lock time. read_snapshot_closed elapsed is total read snapshot lifetime, not rollback latency.

Follow-up status: code correctness independently reviewed without actionable defects; performance objective remains partly unresolved because original mixed-fast regressions cannot yet be excluded. Preserve uncommitted candidate and report this limit. Raw evidence under `target/wallet-2980-read-batch/final/{version.json,binary-comparison.json,samples.json,summary.json,report.md,recheck/}`; old stage1 has36interrupted samples and is not final coverage. Host debug syntheticSQLite/fakebackend/Noop only, disk99–100% and few repeats; no device/network/UI guarantee, upgrade duration/disk peak and general bulk writes unmeasured. Narrow index microfixture31,457,280→25,616,384bytes is not end-to-end timing proof. Independent review/final content freeze recorded beside evidence.

2026-11-02 factor/upgrade verification: no delivered production behavior change.27 three-arm mixedfast cases plus18 separate index-only/control pairs pass exact cleanup/background500k/index/optimistic/probe assertions. First control/task-only/both1万 med250.864/468.157/421.204ms; separate control/index-only250.736/248.132ms. Other sizes and all wait ranges in target/wallet-2980-factor-check/report.md. Current helper restored byte-identically after diagnostic build. Same profile/features/helpers, but binary layout/build time and host I/O remain confounders; task writing is the priority suspect, not proven cause. No rollback implemented this verification round; no new target500k delete timing; old66+8 samples retained.

New ignored SQLx component deletion_migration_timing.rs covers500k synthetic addresses,42-char wallet/35-char UID/50-char address, active/inactive. Exact original/narrowing SQL in BEGIN IMMEDIATE transactions, concurrent reads and same-pool/external writers; all500k identity formulas/index columns verified. DB33pass/1ignored; ignored test3fresh repeats=6upgrades pass. Original writer occupancy0.741–2.073s, narrowing0.690–0.798s; narrowing pooled writes0.691–0.798s/external0.672–0.780s, point-read maxima0.861–9.831ms. Visible DB/WAL/SHM peak423.190/421.245MiB, maximum growth52.889/24.585MiB respectively. Shared-filesystem free decrease53.184/72.383MiB is not process-specific peak;1msfile/2msfree sampling can miss temporary/unlinked files. Full startup/migration metadata excluded; probe window includes commit/probe completion. Short-wallet preliminary measurements separate. Production functions unchanged, API18/helper3 evidence reused. Source/binary provenance, raw45cases,6upgrades, review/finalaudit under target/wallet-2980-factor-check/. No commit/push/release.

2026-11-02 user-approved task-write rollback: restore wallet_deletion.rs byte-for-byte to pre-micro helper36ca78524aa7e382c6a1797f31bf7e4d5aa75c3682edac0ccc294c47faac35f7. Context-based per-request pool and existing single-row create_multi_task RETURNING+returned IDs restored; remove2micro-specific unit tests, keep wire unit and durable/failure/recovery API integration tests. Narrow index/migrations and all stable read/token/reservation/identity/binding/count/atomic rollback/busy retry code unchanged. Supported API build passes; fresh wire1,DB33/1ignored,API18/4ignored pass (API181.63s). Timeout/cancellation/WAL release/partial cleanup recovery/concurrent identity and binding safeguards covered.

Rollback final-source matrix48cases: candidateplain/fast/changed3repeats at mixed100/1000/10000(with500kbackground) and target500k; extra before/afterpairedfast3repeats allsizes. Sequential freshprocess/fixture and alternatingfast order, actualnarrow index/payload/task1-1-10-500/background500k/optimisticfallback/probeoverlap verified. Candidate plain medians(ms) mixed7.085/26.368/208.181,target500k15035.397; fast before→after medians(ms) mixed16.428→6.128 /43.996→44.728 /234.077→232.209, target500k14876.450→15220.418; changed medians12.411/30.487/240.062,target500k16451.818. No broad speedup claim:500kfast2.3%slower this sample and mixed100fast217.934ms slow sample remains afterrollback. Task micro-optimization was not proven root cause; do not attribute all variability to code or host.

Rollback other operations: allcandidate sampled point reads<=0.440ms; mixedfast/changed pooled+external write maximum157.732ms. Target500kfast pooled write waits9.565–11.681s,median9.801s (beforemedian9.448s); substantial blocking remains. Waits include scheduler/pool/gate/SQLite, not pure lock time. Benchmark is debug syntheticSQLite/fakebackend/Noop, excludesfixture+upgrade/network/UI/realdevice; three samples percondition, high disk pressure and layout/cache/IO effects remain. Old results retained separate; no mixing epochs. Evidence `target/wallet-2980-revert-task/{version.json,samples.json,summary.json,report.md,*regression.log,wire-unit.log,api-build.log}`; frozen full diff/new file hashes and independent review/finalaudit beside evidence. No commit/push/release.

2026-11-02 index-maintenance optimization: only forward migration `20261102020000_remove_redundant_account_uid_prefix.sql` removes nonunique(uid,chain_code); retain(uid,chain_code,derivation_path_index) for expansion joins plus the wallet/rowid narrow index. No production function change to deletion/source/proof/retry/transaction/task persistence. No explicit short-index SQL consumer found; actual UID search with10000foreignUIDs stays below1000VMsteps and three-key expansion predicate retains long-index seek. Fresh/populated migration regression passes and preserves identity tuples. DB35/1ignored, accountDAO4, expandrepo3, candidateAPI18/4ignored pass. Cache32MiB and PK-IN experiments rejected; no cache/connection/SQL rewrite delivered.

Final48case matrix after disk recovery, candidateplain/fast/changedx3 at mixed100/1000/10000+target500k; fresh pairedbeforefast3eachsize. ActualUIDshortpresence before1/after0, retainedlong1/narrow columns, cleanup/tasks/payload/background500k+UID, optimistic/fallback and probeoverlap pass. Fast median before→after(ms): mixed26.214→28.378 /67.589→47.818 /260.840→262.445; target500k24970.235→16315.783. Candidateplain medians9.333/28.353/258.952/22214.681ms; changed11.026/33.784/277.992/17747.567ms.500kfast pooled write waits median15.242→9.772s, independent writer12.405→8.218s; after pooled range6.829–12.531s, so substantial blocking remains. Candidateallpointreads<=2.364ms; mixedallwrites<=162.973ms. Three noisy pairs do not establish universal36%speedup or all-scale/no-regression; two mixedfast medians slightly higher. PythonSQLite3.52 VM20.5M→19M is diagnostic only, not SQLx universal counter.

Two interrupted resource-limited batches retained separately (SQLite14 fixture open failure and SQLite13 disk full); not mixed with final48. Finaldriver requires>=3GiBfree percase; observedminimumstart8.588GiB/end8.140GiB, no resources failures. Shared-host CPU/I/O still not isolated. Debug/fakebackend/Noop synthetic only, excludesfixture/migration/network/UI/devices. Performance version.json immutable. Concurrent workspace task later changed wallet creation timing/harness; physical_deletebody reconstructed from oldfixed diff and verified identical, otherdelete dependencies/helper unchanged. Current-source finalAPIbuild and18regressions/4ignored pass225.34s, separately bound final-regression-version.json; no claim48was rerun on rebuilt binary. Evidence `target/wallet-2980-index-maintenance/` includes full samples/summary/report/source-version distinction and independentreview/audit. No commit/push/release.

2026-11-02 normal forward-upgrade check: user retains additive migration and forbids edits to old files.22tracked API-wallet SQL files equalHEAD; all15v2.0.0 and18v2.1.0 historical SQL files byte-match current. Added realSQLx-history startup verification: execute predecessor migrator against empty temp DB, seed existingaccounts/walletbindings, close, open through actualSqliteContext twice. Canonical22→25 and24→25 paths pass; manual authentic release15→25(v2.0.0) and18→25(v2.1.0) pass. Allaccount fields, nonsecretwalletidentity/type/binding, actual UID search, oldversion/checksum/success tuples, newchecksums/exactcatalogcount and idempotentrestart checked. No metadata deletion/checksum bypass/ignore_missing. Runtime and all migrations unchanged. DefaultwalletDB36pass/2ignored; manually released_catalogs_forward_startup1pass with2releasecases. Tests use small syntheticwallet/accountfixtures, not all historical financialdata/versions; no downgrade claim. Evidence/catalogexport/hashes under target/wallet-2980-upgrade-check/. Independentreview/finalaudit recorded there. Uncommitted.
Forward-upgrade final assertions additionally compare original wallet phrase/seed bytes without logging values and require integrity_check=ok on each startup. Latest logs db-verified.log and releases-verified.log bind final test source; normal forward cases pass. No old SQL edit/checksum bypass or downgrade behavior change.


2026-11-03 deletion writer optimization (uncommitted candidate): keep the existing stable read snapshot, original data_version token, BEGIN IMMEDIATE reservation, identity/binding/fingerprint/count validation and atomic rollback. Only ApiAccountDao::delete_for_wallets changes. Inside that reserved transaction, a bound query over the retained narrow wallet index checks for ANY account outside the selected wallet addresses, including inactive and orphan rows. With no outside accounts, omit WHERE/RETURNING and let SQLite clear the account/index B-trees; otherwise keep the existing scoped DELETE. Empty scope remains a no-op. No migration, index, journal, durability, task schema or worker changes. This optimizes exclusive address-table ownership, not arbitrary large deletions with other addresses present. SQLite may retain row deletion when DELETE triggers exist; trigger semantics remain required. Reference: https://www.sqlite.org/lang_delete.html#the_truncate_optimization .

Regression mapping: DAO deletion_tests::deleting_last_wallet_avoids_per_row_index_rewrites checks real SQLite VM work and row count; deletion_preserves_inactive_and_orphan_accounts_outside_scope, all_target_deletion_rolls_back_and_can_be_retried, empty_deletion_scope_keeps_all_accounts, deletion_retains_sqlite_delete_trigger_semantics cover scope/rollback/empty/trigger behavior. Repo api_wallet_deletion_clear_rolls_back_on_later_failure_and_retries fails after the clear and verifies wallet/account rollback and retry while an unrelated empty wallet survives. Existing source-token/gap/new-id/binding/identity/cancel/WAL/lock-retry tests remain unchanged. API exclusive retry/cleanup regression must run via wallet_deletion_exclusive standalone target with explicit no-outside-address precondition; shared OnceCell inventories cannot prove that branch. Existing main deletion suite retains ordinary failure/timeout/cancel/repeated/concurrent/durable/bounded tests. Actual validation and performance evidence will be recorded after completion; do not infer App or real-backend outcomes from these tests.


2026-11-03 host validation for the whole-address-table clear: real Sqlx VM steps/1000 accounts went RED36037 to GREEN6028; DAO5pass, wallet-repo37pass2ignored. Final API default18pass5ignored plus isolated exclusive retry/cleanup1pass. The latter proves outside-account count0 in a fresh integration executable; the first grouped version was rejected as insufficient branch evidence by independent review. Final-source exclusive500000 changed-token supplemental case passed: delete5.595s, pooled writer1.591s, external writer1.608s. Format and diff checks pass. No production API orchestration change.

Host matched A/B:62 passing fresh-process samples (60measured+2warmups), same fixture/probes and original versus candidate frozen binaries, fast/changed-token modes,3repeats. Exclusive500000 median delete/write seconds: fast9.437/5.159 ->4.106/0.262; changed10.841/6.500 ->5.532/1.717. Preserve changed-token candidate external-write slow sample4.935s (pooled4.860s); cause unverified. When500000 target addresses coexist with one other-wallet address, the candidate retains scoped deletion: median external write fast5.259s, changed6.300s, maximum changed7.463s (pooled maximum8.052s); this case is not solved by whole-table clear. With500000 other-wallet addresses, selected100/1000/10000 fast median delete milliseconds baseline->candidate5.07->5.35/21.62->21.62/179.48->180.33; external write3.92->3.89/22.70->22.83/114.81->110.31. Changed-mode mixed samples remain in the raw matrix. No broad speedup claim for retained-account cases. All probes measure scheduling/gate/pool/SQLite execution as applicable, not pure lock time; point reads do not represent all App queries. Synthetic debug data/fake backend, excludes seeding and UI; no real-device/backend claim.

Evidence: target/wallet-2980-write-wait/{db-red.log,db-green.log,db-wallet.log,final-api-tests.log,final-exclusive-tests.log,final-exclusive-500000.log,scale-samples.jsonl,scale-summary.json,scale-logs/}. Cargo relocated-cache aliasing was detected by source guard-literal/executable hashes, rejected builds were not timed; clean workspace package rebuild and frozen distinct original/candidate executables establish accepted provenance. Independent read-only V2 review closed test isolation P2 and independently recomputed all62logs; App/native candidate verification is still pending and must not be inferred from host results.


2026-11-03 candidate App verification: rebuilt unpublished local SDK8d61a7a+the above private DAO change in an isolated copy of Appf66/plugin0be343. Non-SDK Cargo lock789entries unchanged. iOS18.6 x86_64 Debug, same dedicated9203simulator, complete nine-chain500000unique synthetic addresses/0assets, protected empty test-wallet backup restored; all wallet fields unchanged against WAL-aware backup and production missing-chain query0pending for all9chains. No real backend/chain transaction. Built/installed431file hashes and strict simulator signing match; final native framework70692923616c435230f853b6ba48aaccf7b8af6824576ef5316cdc1827ab3d17 contains candidate guard. Temporary static.a and intermediate caches were reclaimed after receipt/final package verification due disk pressure; actual final framework is retained. First Flutter build/installation attempts failed for disk space; only owned regenerable QA caches were reclaimed, then GUI restart restored the test simulator service and final install/start succeeded. No real wallet or other device data was changed.

User completed normal wallet authentication and actual App removal. Native snapshot-open->dispatch18.653s excluding password, vs prior baseline26.505s. Writer occupied6.839s, transaction fingerprint4.939s after concurrent data_version commit (optimistic=false), address DELETE1.885s, commit3ms. External SDK-window reader734samples/max2.717ms; writer108samples/max6.906s;0errors. Prior App writermax15.943s; this is one before/after App trial with the same logical shape, not a strict storage-layout/host-load matched experiment. All121resource samples retain RunnerPID63716; SDK RSSpeak602062848B (574.2MiB), whole-trial620548096B (591.8MiB). API WALfile SDK-window peak885832B, whole-trial4120032B; after probes stopped PASSIVE[0,208,208],TRUNCATE[0,0,0],file0. File allocation/physical layout differs from the prior seeded database; do not infer universal latency or WAL limits.

Final App state: api_wallet/api_account/api_assets0, integrity_checkok,500durable successful cleanup tasks/500000unique addresses,1000max per request,77311Bmax wire body. Local mock saw1device-delete +504unbind calls (4injected503 then automatic retry); watcher observed4durably failed tasks before changing only local mock back to success. Post-delete welcome page visible and language sheet opens/dismisses, same process alive. Read/write probes and local mock stopped; test backups retained. External probes are point database operations, not full other-wallet UI actions. No Android/ARM/release/real-device/real-backend measurement. Evidence target/wallet-2980-write-wait/app-qa/{candidate-package.json,built-app-manifest.json,installed-app-manifest.json,fixture-validation.json,native-perf-source.log,native-deletion-perf.json,db-probes.jsonl,measurement-summary.json,deletion-result.json,mock-failure-recovery.json,ui-observations.json}. Final independent review and version recheck are recorded alongside the evidence in final-review/post-review-audit records; consult their actual status. No commit/push.
