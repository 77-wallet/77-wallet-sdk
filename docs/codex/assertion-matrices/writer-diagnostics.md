# Temporary writer ownership diagnostics

Scope: WALLET-2981 diagnostic release; no SQL, mutex fairness, batch sizing, encryption, endpoint or retry policy change. Default-disabled DB telemetry is enabled only for Context's stable ApiWalletDbPool. Public Rust pool locking methods return an owned WriterGuard wrapper; existing repository/SDK callers use inferred guard types; no FFI signature change.

| Invariant | Evidence |
|---|---|
| Shared wait/acquired/released IDs, real SQLite mutation preserved | writer_events_follow_real_sqlite_lock_lifecycle |
| Cloned pool uses same gate, blocker label known, cancelled waiter does not acquire | cloned_pool_reports_the_holder_and_a_cancelled_waiter |
| Dropping holding future releases original mutex and reports terminal event | cancelling_a_holder_releases_gate_and_emits_release |
| Full/closed queue never awaits and cumulative losses visible; unknown operation redacted | bounded_or_closed_diagnostics_never_block_and_redact_unknown_ops |
| Typed real-pool events reach existing DEBUG envelope with initialization identity, no arbitrary operation text | real_pool_events_reach_app_with_owner_identity_and_without_raw_operation |
| Receiver stops when senders close; absent App receiver does not block it | forwarder_finishes_when_pool_sender_is_gone |
| Existing delayed writer/action split and stored entity/error behavior | upsert_timing_separates_writer_queue_from_action_and_preserves_row; measured_upsert_preserves_closed_pool_failure |

Necessary checks: wallet-database lib writer_diagnostics + write_timing_tests + api_wallet_repo_ filters; wallet-api lib writer_diagnostics and existing creation/upload/notification diagnostic filters. Scope rustfmt and diff checks, plugin fixed-rev integration check.

Limits: full suite and new iOS package/QA-device E2E not covered. Only the DB-to-forwarder queue is bounded; the existing App notification queue remains unbounded. Notification load/memory and instrumentation timing effects are not device-scale validated. Bounded bridge and existing frontend delivery may drop events; acquire/release notifications can interleave after actual release. IDs reset after process restart; raw SQL writers are outside Rust gate ownership. Holder still must be identified from next QA run. DNS system-resolution check on developer machine cannot establish phone connectivity or actual CI config. No performance/root-cause/fix claim from unit counts.
