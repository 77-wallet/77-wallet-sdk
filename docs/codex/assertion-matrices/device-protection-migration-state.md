# Device-protection migration progress

Scope: wallet-database MigrationRepo; existing config table, internal types=1 rows.
No schema changes. State is operational progress, not tamper-proof authorization.
Absence does not mean legacy access is permitted or native keys may be created.

| Scenario | Persisted invariant | Test |
| --- | --- | --- |
| Begin, complete, close/reopen database | DeviceRequired persists | completed_state_survives_reopen_and_cannot_be_restarted |
| Retry/overlap begin and complete after completion | State never regresses to Pending | completed_state_survives_reopen_and_cannot_be_restarted |
| Complete without begin / empty scope | Error; no migration row created | completion_without_begin_does_not_create_state |
| Unknown stored version | get/begin/complete reject; existing value unchanged | unknown_state_is_preserved_and_never_reset |
| Completion races first begin | Completion returns DeviceRequired or NotStarted; never successful Pending | completion_racing_with_first_begin_never_reports_pending_success |

Run `cargo test -p wallet-database --offline --lib repositories::device_protection`.
Component tests use actual SQLite schema initialization and reopen under target; no backend.

Callers must use stable non-secret scope IDs and verify every record in a scope before
marking it complete. New/old-data detection, safe native provisioning, file migration,
and integrating this state into SDK service policy remain separate work. A plain SQLite
row can be externally tampered with or rolled back, so it cannot independently prove a
wallet is protected or authorize fallback to legacy files. No reset API is provided.

## SDK tracked file migration

`KeystoreProtection::migrate_files` now records progress per expected record identity
(SHA256 of versioned context AAD). Explicit caller policy is still required; completed
progress can only strengthen it to DeviceRequired. The method validates supplied inventory
before changing progress, then calls shared migrate_file and marks each successful record.
It returns success only when all supplied records succeed; it cannot certify omitted files.

| Scenario | File + database invariant | Test |
| --- | --- | --- |
| Native failure on second file, DB reopen, retry | First file stays completed/unchanged; second pending resumes; no double wrapping | partial_migration_resumes_after_database_reopen_without_rewrapping |
| Legacy ciphertext restored for completed record | Reject despite caller legacy allowance; completed state preserved | completed_record_rejects_legacy_even_when_caller_allows_migration |
| Empty, duplicate identity/path, missing/nonregular/symlink inventory | Reject before progress writes; original file unchanged | invalid_inventory_does_not_create_progress_or_change_files |
| New protected file without progress | Verify under DeviceRequired; record completion with zero conversions | new_protected_file_is_recorded_without_legacy_conversion |

Run `cargo test -p wallet-api --offline --lib device_protection`.
These component tests use actual SQLite and files/password crypto, with a deterministic
native provider fixture. Inventory discovery, stable production record-purpose conventions,
safe native provisioning and service/App activation remain pending. Callers must exclude
legacy writes and inventory changes during the operation. No whole-wallet or hardware
runtime completion claim is made by this component.
