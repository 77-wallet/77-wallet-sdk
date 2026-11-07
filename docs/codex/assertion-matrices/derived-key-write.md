# Derived key append failure safety

Scope: `wallet-tree` ModernIo::store_subkey, existing synchronous writer.

| Scenario | Expected result | Persisted invariant | Test |
| --- | --- | --- | --- |
| Append with wrong password | Error | Both ciphertext and metadata unchanged; no extra files | failed_append_preserves_ciphertext_and_metadata |
| Append with correct password | Success | Old and new keys decrypt through existing load API; index includes both | successful_append_keeps_both_keys_and_publishes_index |

Run: `cargo test -p wallet-tree --offline --lib file_ops::v2::tests`.
Fixtures use synthetic bytes and temporary directories under target; no backend or device.

This ensures metadata is not published before the existing keystore operation succeeds.
It does not provide atomicity across the two files: metadata writes can still fail after
the keystore is saved. Crash recovery, hardware callback failures and migration require
separate integration coverage. No native protection is activated by this change.
