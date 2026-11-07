# Backend log upload configuration and diagnostics

Transport support is in `wallet_transport_backend::log_upload`. Configuration requires only the complete upload `url`. Credentials are fetched from the same origin at `/log/getKey` using the SDK initialization SN, cached in memory, and are not stored in configuration.

The client sends multipart `key`, `secret`, `sn`, and `file` to the configured URL. The file name is multipart metadata; the server chooses the final object key. Do not infer file transmission from the worker's pending-byte count: credential acquisition happens before the file request is built/sent.

A response succeeds only for HTTP2xx and JSON boolean `success:true`. The file acknowledgement data is not consumed or decrypted. Empty/malformed acknowledgements fail. Connect timeout10s, request timeout60s; response acknowledgement limit64KiB. Redirects are refused. A file business401 refreshes credentials and retries once; other rejections/transport failures are returned to the worker. Authentication, TLS validation and these retry rules are unchanged by diagnostics.

## Configuration

```yaml
log_upload:
  url: "https://logs.example.invalid/log/uploadFile"
```

Missing configuration disables the worker; an invalid configured URL fails initialization. This supersedes this document's former static key/secret configuration example, which did not match the current credential-fetch implementation.

## Automatic worker

The worker ticks at startup and every5minutes. Pending content below1KiB is retained. It reads the current log file through one descriptor and commits its identity/offset only after backend acknowledgement. Failure preserves both in-memory and persisted checkpoint. Checkpoint saving remains best effort. There is no rotated-archive backfill or newly introduced chunking.

## Temporary App diagnostics

Existing DEBUG events carry marker `WALLET-2981-notify-v1`, actual SN, configuration state, cycle and attempt results. A transport failure now also has `failure_detail`:

- `request`: `credentials` or `file`.
- `step`: `send` (request execution) or `response_body` (reading an already obtained response).
- `kind`: reqwest's timeout/connect/body/decode/request classification, otherwise `other`.
- `io_kind`: a recognized standard I/O error category, otherwise `unknown`.
- `cause_hint`: best-effort `dns`/`tls_certificate` hint from known source-error markers, otherwise `unknown`; this is not definitive root-cause evidence.

Only fixed allowlisted categories are forwarded; no source text, URL, credentials, content, response body or local path. HTTP status and backend rejection behavior remain unchanged. HTTP/acknowledgement failures do not gain a transport failure detail. DEBUG delivery remains best effort and does not replay history.

QA supplied a case of approximately15MB pending and a quick generic transport failure; this does not establish a payload-size limit or even that file sending was attempted. First use request/step classification to locate failure, then investigate the indicated endpoint/network or confirmed server limit. Do not bypass TLS or change endpoints/limits on that hypothesis alone.

## WALLET-2981 writer ownership diagnostics

Temporary `component:db_writer` DEBUG events observe the stable Context's `ApiWalletDbPool` gate. Correlate `pool_id` + `lock_id` (process-local, not persistent identifiers), `op`, and `event:wait/acquired/released/cancelled`. `lock_time_ms` is the database event wall-clock time; the common `time_ms` is notification forwarding time, which may be later. `wait_ms` is measured immediately around gate acquisition before the existing tracing call. `held_ms` covers the acquired guard's lifetime, including any work/logging while held. It does not isolate SQL execution.

A cancelled wait reports `cancelled`; dropping/cancelling a holder releases the same Tokio mutex and reports `released`. Release notifications are enqueued after dropping the guard, so concurrent acquisition may be observed before that release notification. Match IDs/timestamps rather than notification arrival order. Direct SQLite writers that bypass this Rust gate are outside these ownership events.

The database-to-App bridge uses a bounded 1024-event queue and `try_send`. Cumulative `dropped_events` reports database queue delivery failures when a later event gets through; absent/locked/closed App senders can also drop notifications, as before. No history replay or complete-delivery guarantee. Events carry only allowlisted static operation labels (unknown labels become `other`), IDs, time and counters; no row data, SQL, raw error/input or callbacks. Telemetry does not await App handling, change transactions or intentionally change mutex fairness/business ordering. Instrumentation and the forwarder add overhead and may perturb timing. The existing App notification queue remains unbounded: the 1024 limit applies only to the DB-to-forwarder queue, not end-to-end memory/notification volume. High-frequency notification load has not been validated on the affected device. Other database pools do not install this bridge.

## DNS failure triage for QA

The beta348 App evidence reports `request:credentials, step:send, kind:connect, cause_hint:dns` after approximately79ms. This precedes file transmission. The pending28MB does not establish a size failure. The hint is a classification, not an authoritative resolver diagnosis.

The plugin embeds `wallet_plugin/rust/config.yaml` at native build time (`build.rs` -> encrypted build output -> `api::get_config` -> SDK `Config.log_upload`). Upload configuration is independent of the selected business-backend environment; `/log/getKey` retains the configured upload origin. Changing the business backend selector does not select a different upload origin.

For this investigation the existing local plugin upload configurations resolved using the machine's configured system resolver, returning non-private/non-loopback addresses. This is not a QA-device or CI-build configuration check. No upload request, credential or wallet data was sent during that DNS check.

QA can keep the same installed build and retry once on another network (for example phone hotspot), keep the App foreground through the next automatic upload tick (up to five minutes), and export the ordinary App log. Compare the existing upload request outcome/detail. If the other network works, investigate the failing network/device resolver; if both fail, compare the actual native package upload configuration with the intended environment, then have the service/network owner check that origin's DNS and availability. These comparisons narrow the cause; they are not proof of a particular DNS defect.

Do not change TLS validation, substitute a fixed IP, change upload origins or blindly add retries based only on this hint. Failed upload checkpoints remain available for the existing scheduled retry. The lock diagnostic update does not claim to repair DNS/upload connectivity.
