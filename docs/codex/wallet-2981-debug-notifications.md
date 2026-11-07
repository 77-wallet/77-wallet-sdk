# WALLET-2981 temporary App DEBUG diagnostics

Authorized by user on 2026-11-04: reuse existing ERR/DEBUG notification channel to investigate API wallet creation latency and missing Rust log uploads. No App flow/API/security behavior changes; use DEBUG exclusively so diagnostic failures do not trigger App error-upload handling. Remove after investigation.

## Tester's workflow

Use a package that includes this SDK revision. Keep App foreground during reproduction and for the next scheduled upload tick (up to five minutes). Export the same App log text already used by QA. Search for WALLET-2981-notify-v1 in DEBUG notifications. Existing package does not acquire this capability until rebuilt/reinstalled.

## Records

Every diagnostic contains UTC Unix time_ms, actual SDK initialization sn, upload_configured, and a temporary marker. No wallet address, name, input/output, password, phrase, proof, key, URL, credential, local path or raw error is forwarded.

- creation: call_id (unique per process), wallet_type, stage, begin/end, elapsed_ms and outcome. Interleaving is correlated per call. A scope dropped before result reports cancelled_or_unwound. Stages nest; do not sum total with its children. Compare App call-start and Rust total-begin to identify pre-execution delay.
- log_upload: configured(enabled, interval_sec), each tick, skip_below_threshold(bytes), request begin/end(bytes, elapsed_ms, outcome, safe error_category and numeric HTTP status), cycle error for local read failures. No change to existing ACK/offset/retry policy. The request interval includes credential acquisition and upload; it does not independently time them.

## Limits

Delivery is nonblocking and best effort. Missing/unsubscribed/closed/busy App channel can drop diagnostics; startup records emitted before Dart subscribes are not replayed. The upload_configured field repeats on creation events so a missed initial configured event does not hide missing upload configuration. Existing App must enable its caution log capture, as in the supplied QA text files. No historical Rust files are replayed, no general tracing bridge, no per-address events. The existing frontend queue remains unbounded; emitted volume is limited to creation phase transitions and scheduled upload events, not every Rust log line. Current change covers coarse creation stages; password validation's internal native/KDF subphases remain in Rust file logs.

## Verification

Focused offline harness compiles the actual diagnostic emitter, create timing, upload worker and offset tracker source and their inline tests against prebuilt dependencies. Context/envelope/path and real HTTP upload client are stubbed in this harness; this is module validation, not real network or native/App E2E verification. Full SDK cargo check is recorded separately. Original App repository remains unchanged. No commit/push/release authorized.
