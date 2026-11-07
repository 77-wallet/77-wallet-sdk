# Device protection — SDK contract batch

Scope: platform-neutral provider contract. Production wallet routing is not connected yet.

| Scenario | Expected invariant | Test |
| --- | --- | --- |
| No provider configured | Both operations return NotConfigured; no pass-through | missing_provider_fails_both_operations |
| Injected provider | Operation, exact input and contextual AAD forwarded; provider result returned | forwards_bound_context_and_returns_only_provider_output |
| Native failure | Typed error preserved; input is not returned as a fallback | errors_propagate_without_returning_input |
| Context binding | Different scope/purpose pairs have unambiguous length-prefixed AAD; empty fields rejected | context_encoding_is_unambiguous_and_rejects_empty_fields |
| Deferred native result | Pending yields to executor, later poll delivers native result | pending_native_callback_yields_before_returning_result |

These fixture tests validate dispatch semantics, not encryption strength. Native adapters must enforce AAD, platform-keystore policy, nonce generation and key lifecycle, and expose their actual hardware/software security level separately. No production storage protection claim until read/write paths are connected.

Validation of contract and storage tests:

```sh
cargo test -p wallet-api --offline --lib device_protection::
```

Production export compilation: `cargo check -p wallet-api --offline`.

## Existing-keystore byte compatibility

| Scenario | Expected invariant | Test |
| --- | --- | --- |
| New protected bytes read under either policy | No migration/re-wrapping; password decrypt works | new_file_never_enters_migration_or_double_wraps |
| Old bytes, wrong password | No hardware wrap or replacement returned | old_file_upgrades_once_only_after_correct_password |
| Old bytes, correct password, then read upgraded bytes | One upgrade, subsequent read returns no replacement | old_file_upgrades_once_only_after_correct_password |
| Device-required wallet sees legacy bytes | Reject downgrade before legacy decrypt | required_policy_and_unknown_formats_never_fall_back |
| Unknown/truncated version or native failure | Error, never legacy fallback | required_policy_and_unknown_formats_never_fall_back |
| Hardware unavailable during migration or wrong context on current data | No opened result/replacement; no rewrap | native_failure_does_not_release_legacy_migration_or_downgrade_current_data |

Tests use real existing wallet_crypto password encryption/decryption and a deterministic hardware-provider fixture. The seal/open methods transform bytes only. Callers must supply durable policy, atomically persist and read back replacement at the existing path, then update migration state. Those call sites are not wired yet; no App rollout/migration completion claim. The earlier unused file-store component was removed per user direction.

| KDF compatibility | Invariant | Test |
| --- | --- | --- |
| Excessive legacy work parameters | Reject before password/native work | excessive_kdf_cost_is_rejected_without_native_calls |
| Current Argon2id, Scrypt, PBKDF2 generation defaults | Accepted by resource policy | current_generated_kdf_profiles_remain_supported |

Resource limits: Argon2 memory <=256 MiB, iterations <=10, parallelism <=16 and memory-KiB × iterations <=1,048,576; Scrypt estimated memory <=256 MiB and N×r×p <=16,777,216; PBKDF2 iterations <=2,000,000. Derived length must be 32 bytes. Files outside these limits are left untouched and require an explicit compatibility/recovery decision; arbitrary historical/imported profiles have not been verified.

## SDK initialization binding

| Scenario | Expected invariant | Test |
| --- | --- | --- |
| Reuse cloned provider | Same Arc identity accepted | provider_identity_distinguishes_clone_replacement_and_disabled_mode |
| Replace provider or enable/disable after initialization | Identity mismatch; Context initializer rejects before WalletManager background startup | provider_identity_distinguishes_clone_replacement_and_disabled_mode |

The unit test covers the identity predicate; constructor wiring is checked by compilation/review, not a live backend/global-context test. WalletManager::new preserves legacy initialization; new_with_device_protection binds the injected provider before Context publication. No runtime setter. The plugin must retain/reuse the same provider object across retries. Wallet keystore I/O still needs to consume Context.device_protection; constructor injection alone does not enable storage protection.

## SDK modern-root codec binding

`KeystoreProtection::store_root` and `load_root` now bind the tree's existing-path async
root operations to the injected provider. AAD uses stable wallet identity as scope and
`root-keystore` as purpose. These methods are not yet called by wallet services.

| Scenario | Expected invariant | Test |
| --- | --- | --- |
| Store, recreate codec, read | WDK v1 persisted at root.keystore; correct root recovered; no rewrap or extra files | root_file_roundtrip_uses_device_codec_and_survives_reopening |
| Wrong password / wrong wallet / missing provider | Respective password/authentication/configuration errors retained; stored bytes unchanged | root_read_errors_preserve_file_and_error_category |
| Write without provider | NotConfigured; old file retained; no extra files | root_missing_provider_write_does_not_replace_existing_file |
| Legacy root supplied to protected read | LegacyForbidden before native operations; file unchanged | root_read_rejects_legacy_without_migration_or_rewrite |

Command: `cargo test -p wallet-api --offline --lib device_protection`.
Real password crypto and temporary files, deterministic provider fixture only; no hardware assurance
from these tests. Tree I/O failures are sanitized as Storage; native/password errors stay distinct.
Protection/decoder failure invariants do not promise rollback after an atomic replacement's directory
sync fails. Competing writes and blocking-worker cancellation require caller lifecycle coordination.
Safe first-use provisioning, service cutover, other file/SQLite paths and durable migration remain pending.

## Common SDK file binding

`KeystoreProtection::store_file/load_file` are the common SDK byte interfaces for all
keystore files. They use shared wallet-tree I/O and seal/open; root methods are only
serialization/filename adapters. They require the caller's expected ProtectionContext,
not an identity supplied by the file. Loaded plaintext uses Zeroizing<Vec<u8>>.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Same SDK API with root, derived and verify paths | WDK format, binary payload recovery, repeated reads do not rewrap, no extra files | common_file_codec_roundtrips_every_keystore_kind_without_rewrap |
| Root ciphertext copied to derived path | Expected derived context rejects it; file unchanged; no rewrap | common_file_codec_rejects_cross_record_substitution |

The tests use synthetic record-purpose labels and a deterministic provider; production
callers still need stable record-identity conventions and activation/migration orchestration.
This binding alone does not switch existing service I/O or protect SQLite fields.

## Common file migration

`migrate_file(path, context, password, policy)` now binds shared rewrite to open.
Policy must come from trusted application state. The return value describes this file
only: true means replacement published, false means current-format password/device
verification succeeded without a rewrite. Neither value marks a wallet fully migrated.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Legacy root/derived/verify paths | Password verified before wrapping; original inner ciphertext retained; WDK file reads; further migrations no-op | file_migration_converts_each_kind_once_after_password_verification |
| Wrong password, DeviceRequired on legacy, missing provider | Distinct errors; existing bytes unchanged; no extra files | file_migration_failure_never_changes_original_or_downgrades_policy |

Tests use real password crypto and filesystem publication but a deterministic provider.
No mobile hardware migration is claimed. Wallet-wide persistent completion state,
provisioning and service activation are still pending. After an uncertain directory-sync
failure, callers must inspect/retry the file before marking migration complete.

## Authenticated application rollout

The password-entry application flow authenticates the wallet password once before entering
the gated ordinary-file rollout. Rollout validates each encrypted envelope and wraps its
existing ciphertext without repeating its password KDF.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Authenticated V2 inventory | Root, derived and verify ciphertexts are device-wrapped once; inner ciphertext remains usable | protection_rollout_migrates_all_v2_files_once_and_preserves_inner_data |
| Rollout receives an irrelevant password value after entry authentication | Migration does not decrypt each file or depend on that value | protection_rollout_wraps_authenticated_inventory_without_per_file_password_kdf |
| Wrong password at application entry | Reject before provisioning or file publication | protection_password_entry_activates_v2_once_without_extra_frontend_call |
| Interrupted publication and retry | Published files remain protected and are not wrapped twice | protection_rollout_resumes_published_file_after_progress_write_failure |

## WALLET-2964 READY checks and API writer isolation

Normal active/READY password validation retains the existing file gate and native
key usability probe, but must finish independently of the API business writer
gate. This does not waive the writer gate for first provisioning or file migration.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Real protected wallet; expansion repository owns API writer gate | Correct password succeeds and wrong password fails before writer release; expansion still commits its item; no reprovisioning | ready_password_entry_finishes_before_expansion_releases_api_writer |
| READY native probe paused | Other API writers acquire the gate; file mutators remain excluded; native key is not recreated | ready_native_probe_retains_file_gate_but_not_api_writer |
| Missing key state / PENDING state; API writer held | No provisioning or state publication until release; guarded initialization then reaches READY | missing_and_pending_initialization_wait_before_provisioning |
| READY key with legacy files awaiting migration; API writer held | Files and active marker remain unchanged before release; full migration succeeds afterward | ready_key_does_not_bypass_legacy_migration_writer_gate |
| Native missing/locked/invalidated/timeout/platform failure during READY login | Exact native error propagates without waiting for API writer, reprovisioning or changing files; retry can recover | ready_native_failures_propagate_without_waiting_or_reprovisioning |

Target: `CARGO_TARGET_DIR=target cargo test --offline -p wallet-api --lib unlock_contention_tests -- --nocapture --test-threads=1`.
Reuse existing `protection_initialization`, `protection_legacy_initialization`,
`protection_password_entry`, `protection_startup` and `protection_rollout` filters
for initialization, interrupted migration, token, missing-provider and native-failure
compatibility. All use temporary SQLite/files and synthetic native providers;
phone hardware and the incident's three-minute delay require device acceptance.

## WALLET-2964 unlock diagnostic timing

New INFO diagnostic events contain only fixed operation/phase labels, elapsed
milliseconds and process-local Tokio task/parent task IDs. No function arguments,
passwords, proof/ciphertext, wallet/device identifiers or raw errors are added.
The existing logger envelope is unchanged. A phase end records scope exit even
on failure; it does not assert authentication success. Intervals are nested,
not additive. Task-start events also measure dispatch delay before an owned task
starts, so scheduler time is not mistaken for password decryption time.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Real wallet success, wrong password and native failure | Required stages close on every exit; initialization task links to validation parent; diagnostic fields contain no input/error material; outcomes and provisioning count unchanged | unlock_timing_reports_safe_correlated_success_and_failure_phases |
| Native probe paused while validation and initialization contend | SETUP/file wait begin records appear before controlled hold; end measurements include that wait; original serialization preserved | unlock_timing_measures_controlled_setup_and_file_waits |
| Public caller aborted during native callback | Owned validation continues; its total does not end at caller cancellation; phase ends occur only after owned work and the subsequent SETUP owner complete | unlock_timing_caller_abort_does_not_end_the_owned_validation_early |

Target: `CARGO_TARGET_DIR=target cargo test --offline -p wallet-api --lib unlock_timing_tests -- --nocapture --test-threads=1`.
Reuse the existing unlock-contention and lifecycle regressions above. Instrumented
SDK-only tests do not update an installed App or establish the current simulator
delay's cause; a rebuilt diagnostic Debug/Profile comparison is still needed.
