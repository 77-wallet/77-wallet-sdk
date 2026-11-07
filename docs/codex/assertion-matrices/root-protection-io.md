# Modern root keystore async I/O boundary

Scope: `ModernIo::store_root_with_protection` and `load_root_with_protection`.
Production callers still use the synchronous methods; this batch does not enable native protection.

The implementation is now shared in public `file_ops::protected::{store,load}`.
Root methods only serialize root data/select root.keystore and delegate; they no longer
implement encryption or filesystem operations. The shared API accepts an existing path
and opaque bytes, and its decoder can return any data type. It contains no wallet-record schema.

| Shared I/O scenario | Invariant | Test |
| --- | --- | --- |
| root.keystore, key0.keystore and verify via identical API | Arbitrary binary payload roundtrips through existing password crypto and outer callback; exact requested filenames; no temp files remain | same_io_handles_root_derived_and_verify_as_opaque_bytes |
| Protection fails for each filename | Existing bytes unchanged for every record kind | failed_protection_preserves_each_existing_file |

Run all shared/root regressions: `cargo test -p wallet-tree --offline --lib file_ops`.
This refactor provides the common entry point; existing derived/verify service callers have
not yet been switched from KeystoreBuilder and must not be described as protected.

| Scenario | Result / persisted invariant | Test |
| --- | --- | --- |
| Asynchronous protect and decode | Existing root.keystore filename; only transformed bytes on disk; existing password decryptor recovers RootData; no temp files | root_roundtrip_uses_existing_password_format_and_path |
| Protection callback fails | Error propagated; original ciphertext unchanged; no extra files | failed_protection_keeps_existing_root_and_creates_no_files |
| Decoder fails | Error propagated; file unchanged; no rewrite or legacy fallback | failed_decoder_never_changes_existing_root |
| File exceeds size bound | Decoder not called; file unchanged | oversized_file_is_rejected_before_decoder |
| Protection returns empty bytes | Rejected before disk write; original file unchanged | empty_protection_result_is_rejected_without_truncating_root |
| Atomic publication fails | Existing destination untouched; temporary ciphertext cleaned up | failed_publication_cleans_temporary_ciphertext |
| Protection suspends, then succeeds | Old file untouched while awaiting; then replaced; no temporary files left | replacement_waits_for_protection_before_touching_old_root |

Validation: `cargo test -p wallet-tree --offline --lib file_ops::v2`.
Fixtures contain synthetic bytes only, use real password encryption, and write under target.
The test marker is not encryption and provides no hardware assurance.

Caller obligations: bind a trusted device codec to a stable wallet context, provision only after
checking protected-record state, serialize competing writes, and enforce durable migration policy.
The load callback must validate format/password and return RootData; reads never migrate automatically.
Do not activate the new writer before all other root consumers support its format.
Derived/verify files, password changes, SQLite secrets and wallet App integration are outside this batch.

Atomic rename protects the old file before publication. A parent-directory sync failure after rename
reports an uncertain durability outcome; cancellation cannot stop an already running blocking write.
This is not a multi-file transaction or crash-recovery implementation.

## Shared migration replacement

`protected::rewrite` reads existing bytes and asks the caller for a validated optional
replacement. Some is published using the same atomic writer as store; None performs no
write. Password/version decisions and durable migration state remain outside wallet-tree.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Replacement followed by None | First replacement persisted, second call does not rewrite; no extra files | rewrite_commits_replacement_and_skips_current_bytes |
| Transform rejected | Original bytes and directory entries unchanged | rejected_rewrite_keeps_original_file |
| Two overlapping transforms | Second reads the first committed version, including case-alias filename on case-insensitive FS | concurrent_rewrites_observe_the_previous_committed_version |

Shared store/rewrite operations serialize by canonical parent directory in this process.
The publication worker owns the lock guard, so cancelling its waiter does not unlock a
still-running write. Callbacks must not await another store/rewrite in that directory.
Legacy writers outside the common API and other processes are not protected by this lock;
service migration must exclude those writers. This is not cross-process locking or a
durable migration-state implementation. Same-directory writes are conservatively serialized
to cover case-insensitive aliases; independent directories can proceed concurrently.

## Read-only existing-file inventory

`file_ops::inventory::scan_wallet` discovers modern and legacy root/subs records;
`scan_verify` discovers the separate App-root verify file. These functions never read
secret contents or change files, migration progress or native keys. Empty results are
not permission to provision a key. SQLite secrets and authoritative DB membership
must be checked by the caller before rollout. Unknown files (including orphan temporary
files) currently block scanning and require recovery; they are never silently discarded.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Mixed modern/legacy files and verify | All six wallet identities found; only public derived_meta excluded; contents unchanged | scans_all_existing_modern_and_legacy_keystores_without_reading_contents |
| Unknown file or different wallet root | Error, original bytes preserved | unknown_file_and_wrong_wallet_root_are_not_silently_skipped |
| Empty existing directory | Empty result; no files or progress created | empty_directories_do_not_create_data_or_progress |
| Symbolic link | Error without following/replacing link target | symlink_is_rejected_without_following_or_replacing_it |
| Different names decoding to one identity | Error, both files preserved | duplicate_decoded_identity_is_rejected_without_changing_files |

Validation: `cargo test -p wallet-tree --offline --lib file_ops`.

## Standard-wallet phrase service integration

Scope: actual `WalletService::get_phrase`, modern WDK root read with injected provider.
The service delegates root loading to `WalletApplication::load_root`; migration policy
and reader selection live there, while `keystore_format::has_device_header` owns the
format probe. This extraction preserves existing behavior and uses the same service tests.
This read path does not enable writes, provision keys or migrate legacy data. Other
business consumers still require integration before wallet-wide activation.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Protected root, new Context on same directory, wrong password and native Locked | Actual service returns phrase before/after reopen; typed password/Locked errors propagated; file unchanged | protected_phrase_uses_actual_service_after_context_reopen |
| Legacy root then completed modern migration with missing modern file | Legacy initially readable without native calls; completion forbids fallback to retained old files | legacy_phrase_stays_readable_but_completed_migration_blocks_rollback |

| Modern password-only root, unsupported WDK version | Existing modern reader works with zero native calls; unsupported WDK fails without fallback or rewriting | modern_password_only_phrase_uses_existing_reader_without_native_calls |

Validation: `cargo test -p wallet-api --offline --lib protection_tests`.
These are offline service component tests with real SQLite/password crypto and synthetic
native envelopes, not hardware or wallet App runtime tests. No global Context is initialized.

## Root seed consumers and password fallback

`get_seed` delegates to the same root loader used by phrase export. Derivation import,
account/chain seed pass-throughs and derived-address listing use that entry. The standard
wallet branch of `try_decrypt_wallet_db` also uses the root loader. Only password failures
continue the existing fallback; typed device failures and policy/state failures propagate.
This does not activate protected writes or change the existing password_proof fast path.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Protected seed and password fallback, then wrong password and Locked | Exact seed recovered; valid password accepted; wrong password false; Locked propagated and root bytes unchanged | protected_seed_and_password_fallback_use_device_reader |
| Legacy password fallback | Correct password true, wrong password false; zero native calls | legacy_password_fallback_preserves_success_and_wrong_password_behavior |

Validation: `cargo test -p wallet-api --offline --lib protection`.

## Root password change through AccountDomain

`AccountDomain::set_root_password` keeps the wallet existence check and delegates to
`WalletApplication::change_root_password`. Existing-root read/write selection shares
`record_requires_device`. WDK records use the generic codec `change_file_password` under
the common directory rewrite lock; the existing password generator transforms opaque
plaintext in memory, and only validated outer ciphertext reaches the atomic publisher.
Legacy records retain their previous writer. No keys are provisioned and no migration
state is changed by this operation.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Native protect failure, incorrect old password, then successful change | Failure preserves original bytes and old-password access; success retains WDK and identical phrase/seed; old password fails; no extra files | root_password_change_preserves_wdk_and_recovers_from_native_failure |
| Two simultaneous changes with same old password | Exactly one succeeds; winning password reads original phrase; no extra files | root_password_change_preserves_wdk_and_recovers_from_native_failure |
| Completed root removed with old root files retained | Password change fails without modifying old files | completed_root_password_change_rejects_legacy_rollback |
| Legacy root change | New password reads phrase, old password fails; existing names retained and zero native unwrap calls | legacy_root_password_change_keeps_existing_format |

Validation: `cargo test -p wallet-api --offline --lib protection`.
The atomicity guarantee is per protected file, before publication. The existing App-wide
password change updates multiple records sequentially and is not made transactional by
this batch. Failures after atomic rename can still report uncertain durability, as above.

## Derived account-file password change

The actual `AccountDomain::set_account_password` entry delegates to the same generic
protected password transform as root changes. Expected context is wallet address plus
`derived:{account_id}`, at the existing `subs/key{account_id}.keystore` path. Read-policy
selection is shared as `record_requires_device`. Protected payloads are not parsed or
rebuilt, and this path never writes `derived_meta.json` or deletes the original first.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Multi-key protected file: wrong password/native failure, then successful change | Exact plaintext retained; WDK retained; index and other account file byte-identical; failures preserve original; old password fails after success | derived_password_change_preserves_payload_index_and_other_accounts |
| Completed account restored to password-only file | LegacyForbidden; ciphertext and index unchanged | completed_derived_password_change_rejects_legacy_rollback |
| Existing password-only V2 file | New password recovers same payload; old password fails; no native calls | password_only_derived_change_remains_compatible |
| Legacy single-address password entry on a protected grouped account | Routes by account id through the common password boundary; WDK remains and every key in the grouped file uses the new password | protected_sub_password_change_updates_the_grouped_account_file |

Validation: `cargo test -p wallet-api --offline --lib protection`.
Fixtures use the existing ModernIo writer to produce actual grouped account payloads.
This covers account-file password changes including the legacy single-address service entry,
but not private-key read activation, multi-file transactionality, or the pre-existing V1
`load_account` unimplemented branch. No new protected-file activation is introduced.

## Legacy algorithm-upgrade recovery

| Scenario | Invariant | Test |
| --- | --- | --- |
| V2 strategy published, only part of obsolete V1 data remains, legacy marker still set | Verify all referenced V2 root/account/verify records before deleting the remaining V1 source; clear the marker only after cleanup; wallet stays readable | protection_password_entry_resumes_partially_cleaned_published_v2_upgrade |
| Only API-wallet ciphertext remains while the legacy marker is set | Wrong password is rejected before format changes, verify-file creation, device provisioning or marker removal | protection_password_entry_rejects_wrong_password_for_api_only_legacy_state |

## Derived private-key reads

The domain bulk-export and individual-key read paths now delegate storage reads to
`WalletApplication::load_account_data` / `load_sub_key`. Both share protected-account
map decoding through the existing public KeyMeta codec, common WDK/password codec and
record policy. Key selection requires the exact chain/address/derivation tuple. Chain
output conversion and legacy file readers remain unchanged. Protected malformed metadata
or collisions between decoded identities fail instead of silently skipping entries.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Actual bulk export domain with locked provider | Typed Locked before chain conversion; stored bytes unchanged | private_key_export_propagates_device_failure_before_conversion |
| Plain V2 then protected grouped account | Identical full key map and exact selected key; wrong derivation cannot select another key; wrong password/Locked preserve ciphertext/index | derived_private_key_reads_match_exact_metadata_and_preserve_files |
| Invalid metadata, duplicate JSON names or distinct names decoding to one identity | InvalidFormat; no rewriting or legacy fallback | malformed_protected_account_payload_is_rejected_without_rewrite |

Validation: `cargo test -p wallet-api --offline --lib protection`.
These are offline component checks, not transaction signing, live-chain, or hardware tests.
The existing algorithm-upgrade path still directly reads legacy subkeys intentionally;
new-user write activation and migration ordering remain outstanding.

## Create/reimport root persistence

`WalletService::create_wallet` delegates root persistence to `WalletApplication::store_root`
and only then calls `restart_existing_wallet`. Existing WDK/completed roots use common
protected store without clearing the directory. Legacy/new unprotected roots retain the
old writer until explicit provisioning and rollout are connected.

| Scenario | Invariant | Test |
| --- | --- | --- |
| Protected root overwrite with native failure then success | Failure preserves old bytes and readability; sibling survives both paths; success remains WDK/readable with no temp files | root_store_preserves_existing_protection_and_files_on_failure |
| Initial legacy root write | Existing legacy reader works; native remains unused | root_store_retains_legacy_creation_until_activation |

Validation: `cargo test -p wallet-api --offline --lib protection`.
This does not activate protected storage for new records or make create/reimport atomic
across DB and derived files. Derived bulk writes still require integration before full
wallet reimport can be described as device-protected. The legacy set_sub_password service method
has no callers in SDK API, plugin Dart or App Dart, but now delegates to the grouped account
password boundary so direct SDK use cannot bypass WDK.

## Derived bulk application writes

- Four live service callers share `WalletApplication::store_subkeys`: wallet creation/import, derivation-path import, account creation, chain synchronization.
- Existing protected account maps merge new entries under the common codec rewrite lock. Old keys and unrelated account ciphertext survive; a protected root requires outer encryption for newly created derived files.
- Native protection failure before publication leaves that file and its index unchanged. This is not atomic over all files in a bulk request; earlier completed account writes can remain after a later failure.
- Updated index entries come from the full resulting key map; retries do not duplicate entries. A malformed existing index fails before ciphertext writes. A missing known protected account is an error, never rebuilt from a partial input.
- Regression tests cover native failure, append/new file, idempotent retry, concurrent appends, malformed index and legacy compatibility. Cancellation test requires an accepted operation to finish its ciphertext/index write while retaining serialization even after its waiter is cancelled.
- Legacy deletion/algorithm-upgrade writers and separate processes do not participate in this application gate; rollout still requires their integration or exclusion during migration. Process termination between ciphertext and index publication remains a recovery boundary; this change does not claim a cross-file transaction.

## Legacy algorithm upgrade before device migration

- Actual `upgrade_algorithm` regressions reproduce two pre-existing hazards: wrong root password returned success and continued cleanup; a mixed legacy/protected root was overwritten by the legacy upgrade writer.
- Upgrade now rejects any device migration progress row, including Pending or records whose file is missing, and scans all wallet root/derived files plus app verify before writing. WDK prefixes of unsupported versions are also blocked. Unknown inventory entries fail closed.
- Root read/write errors propagate; the modern replacement must decrypt to the same seed/phrase before the legacy source can be scheduled for deletion. Failure tests assert source bytes, protected bytes and configuration remain unchanged; successful V1 conversion remains covered.
- Upgrade shares the application write gate with root and derived writes/password changes. Deletion paths and external processes are not yet covered by this gate; no concurrent automatic hardware rollout is activated here. Later-stage failures may leave modern copies beside original files; this is not a multi-file transaction.
- Offline validation: `cargo test -p wallet-api --offline --lib protection`.

## Intentional account deletion

- Durable `RecreationAllowed` progress represents user-authorized deletion, not proof of completed deletion. It remains a device requirement; it grants neither legacy fallback nor native key provisioning.
- SQLite tests verify reopen, begin preserving authorization, verified completion revoking authorization, first authorization without prior migration row, invalid scopes and preservation of unknown versions.
- API integration must require both the target file and its public index entry to be absent before protected recreation. Accidental loss while DeviceRequired must remain blocked.
- Account deletion shares the existing writer gate and finishes an accepted operation even if its waiter cancels. Index/path validation and durable intent precede file removal. The real service calls storage deletion before removing business DB records.
- API regressions cover delete/retry/recreate with an otherwise legacy root, persistent device requirement after recreation, accidental subsequent loss, malformed index, SQLite intent-write failure, retry after file-only deletion, forbidden legacy migration under recreation permission, and old-format deletion compatibility. Other account files remain byte-identical.
- State/metadata failure after file removal may leave a partial deletion; retry is supported. Business DB/backend operations are not in a filesystem transaction. Whole-wallet deletion and external writers remain separate rollout work.
- Review found a cancelled protected password-change publisher could outlive the application gate and resurrect a deleted account file. The protected password task now owns that gate through completion; its cancellation regression first failed, then checks publication precedes deletion and removal stays complete.

## Whole-wallet deletion/reset and reimport

- Single-wallet deletion and both reset services use one storage boundary before business DB removals. The physical reset epoch still advances first. No hardware key is deleted; no cross-filesystem/DB/backend transaction is claimed.
- Snapshot strict files, metadata and DB identities (including inactive wallets) before mutation. Save recreation authorization for protected roots/known accounts before removing any directory. Keep the wallet base and unrelated directories intact.
- Root reimport retains device protection, requires explicit recreation permission if its protected file is missing, and revokes permission after successful publication. Root writes/password changes retain the shared gate when their waiters cancel.
- Regressions cover root + multi-account deletion/reimport, intent-persistence failure preserving ciphertext, reset preserving unrelated data, cancelled root publication ordering, and inactive missing-wallet reset recovery. Partial deletion/DB failures remain retryable storage operations, not a global transaction.

## Explicit empty-storage key initialization (not rollout)

- Provider provisioning is a separate, default-unsupported operation. Existing providers and protect/unprotect behavior remain unchanged; no default enablement is inferred from callback presence.
- Empty-storage checks cover all standard/API wallet and account rows, migration history, strict wallet files, stale public indices and app verify. Unknown initialization versions/types or read errors reject provisioning.
- Persist provisioning intent before calling native code. A non-secret challenge roundtrip precedes key-ready state. READY retries only test the existing key; KeyMissing/Locked/invalidated errors must not trigger provisioning. Pending retries require empty storage again and an idempotent native provisioner that never resets existing keys.
- Tests cover empty initialization/repeat, readiness across context reopen, failed probe then retry, no reprovision after key loss, existing standard/API storage/history/verify rejection, unknown state/type, and default unsupported adapters.
- This boundary is not yet wired to plugin provisioning or automatic activation. The challenge proves provider roundtrip usability, not hardware attestation or recovery of existing wallet ciphertext. Native hardware enforcement remains the adapter's responsibility.

## Explicit legacy-storage key initialization (not rollout)

- Password-bearing initialization scans strict ordinary-wallet inventories, DB/index references, password proofs and existing API phrase/seed packages before provisioning. Existing WDK records, migration history, missing roots/files, unknown formats and orphan API accounts reject initialization.
- Modern account payloads use the existing strict decoder; every DB/index KeyMeta must reference a nonempty key. Password-decryptable but incomplete payloads fail before native provisioning. Decrypted buffers are cleared after verification.
- Regression coverage includes V1/V2 success, wrong password, native probe failure/retry, protected files, missing index/DB targets, missing payload keys, migration history and existing API packages. Existing ciphertext remains unchanged; this operation does not migrate or activate protection.
- API writes and application file writes are gated during preflight. Startup integration must still exclude other writers; this is not cross-process synchronization. READY retries only probe the existing key and are not password authentication.
- Validation: `cargo test -p wallet-api --offline --lib protection`.

## Ordinary V2 file migration orchestration

- One owned application task holds the file and API writer gates across snapshot, initialization, validation, progress and publication. Caller cancellation does not release gates early.
- Reuse strict DB/index/file inventory and exact KeyMeta checks. Reject V1 or mixed legacy identities before provisioning; existing V1-to-V2 conversion remains a prerequisite. Validate every file's password and payload before the first replacement.
- Save all expected file scopes before publication. On retry, progress without a file is an error, including unindexed verify files; explicit deletion's RecreationAllowed is the sole absence exception. READY uses the same native key, never provisions again.
- Tests cover root/all derived files, unchanged inner phrase, repeat calls with byte-identical WDK output, wrong-password preservation, publication followed by progress-write failure and successful retry, legacy format rejection and missing protected verify detection.
- Successful migration sets the versioned ordinary-file active policy; later root and derived writes are protected. This explicit boundary is not yet invoked by startup/unlock and does not migrate SQLite. API packages/password proofs are only verified by first-use initialization here. Progress remains operational SQLite state, not a tamper-resistant trust anchor.

## Default ordinary-file writes after explicit activation

- Active policy is persisted only after complete file validation/migration. Active readers reject legacy fallback even without a per-record marker; unknown policy versions/types and per-record state still fail closed.
- Fresh roots require no per-record state, no wallet/account DB references, an empty strict wallet inventory and no stale derived index. Explicit creation records retry permission before protected publication; accepted writes complete the device requirement. Existing missing DeviceRequired roots remain blocked.
- A first protected derived creation records `CreationAuthorized` before publishing bytes. That state may resume only while both the file and its metadata entry are absent, and it cannot overwrite older progress. Existing-file rewrites still use `Pending`; a missing `Pending` file remains blocked because its loss is ambiguous.
- Tests cover empty activation, failed first native root/derived writes and safe retry, restart before first derived publication, WDK output and durable derived policy, active rejection of newly injected legacy records, and missing Pending derived loss preserving the index. Initialization cannot provision when active policy survives but initialization state is absent.
- This policy becomes operational when the SDK startup/unlock integration invokes the migration boundary. Plugin callback availability alone does not invoke it; no App rollout or hardware integration result is implied.

## SDK startup and existing password entry activation

- Manager startup invokes the readiness boundary before unlock-session rotation and wallet workers. READY only probes the existing key; fresh/Pending storage defers provisioning to a real-password entry. Active storage without READY is rejected; no key recovery/reset is performed.
- Existing WalletApplication password validation (shared by standard-wallet services and API-wallet password caching) now owns serialized setup through validation, format preparation and migration. Context-local session tokens cannot bootstrap inactive protection. No extra Flutter business/UI call is required.
- Real passwords use the existing validation rules before activation. V1 data uses the existing conversion; empty ordinary storage selects V2 after safe initialization without creating a synthetic verify file. Active validation checks existing key usability without rewrapping the inventory.
- set_passwd_cache creates its session/system-ready state only after this flow returns successfully. The failure regression calls that service and asserts native failure preserves file bytes and leaves no session. Other regressions cover startup deferral/key loss, actual V2 password activation/repeat, first-install protected creation, token gating, V1 upgrade and no-provider legacy compatibility.
- An owned setup task continues accepted storage work after caller cancellation, while its caller cannot publish an unlock session until success. No-provider callers keep legacy behavior only when no durable protection initialization/active state requests hardware.
- Validation: cargo test -p wallet-api --offline --lib protection. SQLite API packages/password proofs still need outer protection; this step activates ordinary-file storage only. Full App and hardware validation are separate from these fake-provider SQLite/file regressions.

## Password-rotation retry

- The existing password proof authenticates the old password before rotation starts. If an interrupted retry reaches a protected ordinary file that already uses the requested new password, the file is validated and left byte-identical; files still using the old password are rewritten normally.
- API-wallet rows use the same retry rule for their phrase and seed pair. Both fields must decrypt with the new password before an already-rotated row is skipped.
- Regression coverage includes a protected file already published with the new password and verifies that retry neither rewrites it nor accepts an unrelated password.
