# Backend log upload — batch 1 transport

Scope: wallet-transport-backend only. Existing automatic/manual OSS upload is not
switched in this batch. Backend contract confirmed by user: same envelope as existing
interfaces; HTTP 2xx and success=true required. Response data is unused.

| Invariant | Offline test |
| --- | --- |
| POST multipart has key, secret, runtime sn and exact file bytes/name | multipart_contains_auth_device_and_exact_file_content |
| Invalid endpoint/auth/file metadata fails before network IO | invalid_config_and_metadata_are_rejected_before_network_io |
| Debug does not expose auth | config_debug_does_not_reveal_credentials |
| HTTP failure, redirects, business rejection, empty/malformed JSON cannot acknowledge | acknowledgement_requires_http_and_business_success |
| Actual upload accepts success, refuses 307/308, rejects oversized chunked ack | upload_executes_against_fake_backend_and_refuses_redirects_and_oversized_ack |

Client disables redirects, has 10s connection/60s request timeouts, bounds acknowledgement
body to 64 KiB, and emits no request or response content in errors. HTTPS should be used
for deployment; HTTP remains accepted for the endpoint supplied by the backend.

Run: `cargo test --offline -p wallet-transport-backend --no-default-features --lib log_upload`.
The transport test binds an ephemeral 127.0.0.1 port and may require sandbox permission.
Never run these checks with production credentials or a remote endpoint.

## Batch 2 — wallet-api automatic uploads

| Invariant | Test |
| --- | --- |
| Incomplete upload config is rejected rather than ignored | log_upload_config_rejects_incomplete_credentials |
| New section parses; missing section disables uploads; OSS is not required | log_upload_config_is_optional_and_does_not_require_oss |
| Failure preserves offset, identity and on-disk checkpoint; retry sends identical pending bytes and runtime SN | failed_upload_preserves_checkpoint_and_retry_sends_same_pending_bytes |
| Rotation changes checkpoint only after acknowledgement | rotated_file_commits_new_identity_only_after_acknowledgement |
| Under 1 KiB causes neither upload nor checkpoint change | less_than_one_kib_waits_without_upload_or_checkpoint_change |
| No log_upload config means no background task or OSS fallback | missing_config_starts_no_background_upload_task |

Run: `cargo test --offline -p wallet-api --no-default-features --lib log_upload_config`
and `cargo test --offline -p wallet-api --no-default-features --lib infrastructure::log::upload_log`.
Worker tests use an in-memory fake uploader and temporary synthetic files; no sockets.

## Batch 3 — manual uploads and OSS removal

| Invariant | Test |
| --- | --- |
| Manual upload reads the exact validated file and propagates backend rejection | manual_log_upload_reads_exact_file_and_propagates_backend_failure |
| Existing source confinement and destination-name validation remain active | upload_log_file_source_* / upload_log_file_name_* |

`wallet-api` no longer contains `wallet_oss`, `OssConfig`, or an OSS client. Manual
uploads require configured `log_upload` and return an error if it is absent.


## WALLET-2981 transport failure refinement (2026-11-04)

Scope2026-11-04: repair loss of transport request/step classification; no auth/TLS/retry/offset/endpoint/body changes.

| Contract | Evidence |
|---|---|
| Failed credential acquisition identifies credentials/send and no file request follows | credential_transport_failure_is_identified_before_any_file_request |
| Failed file send distinguished from credential call; secret values omitted | file_transport_failure_is_distinct_from_credentials_and_response_body |
| Truncated file response distinguished from send failure; response text omitted | response_body_disconnect_has_separate_safe_diagnostics |
| Source error markers produce only fixed categories; DNS/TLS are hints | transport_source_diagnostics_never_return_raw_source_text |
| DEBUG mapping keeps existing transport/timeout category and whitelists every detail | detailed_transport_failure_retains_the_transport_category; transport_details_allow_only_fixed_categories_and_omit_unknown_text |
| Failed upload retains checkpoint; successful retry advances only after ACK | failed_upload_preserves_checkpoint_and_retry_sends_same_pending_bytes; rotated_file_commits_new_identity_only_after_acknowledgement |
| Credentials cached; business401 refreshes once, other failures do not | existing log_upload backend tests |
| Redirects refused, bounded ACK, multipart auth/content unchanged | existing log_upload backend tests |

Tests use fake/loopback only. No actual QA phone connectivity, server-size limit or slow-wallet fix claim. No real logs uploaded by these tests.
