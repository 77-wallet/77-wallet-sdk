# Backend Message Compensation

| Test layer | Entrypoint | Expected backend call | Expected result | Failure invariant |
| --- | --- | --- | --- | --- |
| Live backend | `affected_client_can_query_unconfirmed_messages` | `POST sendMsg/queryUnConfirmMsg` with the affected simulator client ID | The authenticated request completes and the response decodes | Timeout or transport failure fails the test; response payload is not logged |
| Live backend | `affected_client_can_request_expired_ack_resend` | ECDH initialization, then `POST awallet/aw/msg/ackExpired/resend` with the affected simulator client ID | The encrypted request completes successfully | Timeout, encryption, or transport failure fails the test; backend configuration is loaded only from live-test configuration |

These tests are gated by `online-tests`, marked ignored, and must be run manually against an
explicitly configured backend. They do not run in the offline default validation set.
The client ID and SN are fixed identifiers belonging to the dedicated simulator fixture; they are
not production credentials.

Known gap: the collector's local persistence and retry behavior after `sendMsg/confirm` failure is
not covered by this live endpoint check.
