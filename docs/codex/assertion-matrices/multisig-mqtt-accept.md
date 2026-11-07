# Multisig MQTT acceptance conversion

Scope: MultiSignTransAccept -> NewMultisigQueueEntity, before database writes.

| Input | Required result | Regression |
| --- | --- | --- |
| Valid signer status and bill kind | Preserve queue/from/signer/status | valid_message_preserves_signer_and_transaction |
| Invalid signer status | Return error, never produce a queue entity | invalid_signature_status_returns_error |
| Invalid bill kind | Return error, never produce a queue entity | invalid_transfer_type_returns_error |

The existing task error/retry/cleanup path remains unchanged. These unit tests
exercise the pure conversion boundary; they do not claim dispatcher panic
recovery or database integration coverage.
