# API Wallet Business Reminder Assertions

## Scope

The `api_business_notification` table is in `api_wallet.db`. It stores API wallet
message-center reminders such as outgoing address balance insufficiency. Withdrawal
review remains in the withdrawal order and review list; it does not create a row here.

## Durable Facts and Invariants

| Fact or action | Invariant | Test entrypoint |
| --- | --- | --- |
| API wallet migration | The table exists in `api_wallet.db`, never in `data.db`. | `wallet_database::repositories::api_business_notification::tests::core_database_does_not_create_api_business_notification_table`; `notification_survives_reopen_and_is_scoped_by_uid` |
| UID-scoped reads and updates | A UID cannot list or mark another UID's reminder read. | `wallet_database::repositories::api_business_notification::tests::duplicate_delivery_preserves_read_and_resolution_state`; `wallet_api::service::api_wallet::business_notification::tests::balance_reminder_list_and_read_are_uid_scoped` |
| Duplicate delivery | Same event ID replay preserves payload, time, read and resolved facts; a different event ID creates independent history even with the same business key. | `wallet_database::repositories::api_business_notification::tests::duplicate_delivery_preserves_read_and_resolution_state` |
| Read and resolution | Reading a reminder does not resolve its active condition; resolved reminders remain in history. | `wallet_api::service::api_wallet::business_notification::tests::balance_reminder_list_and_read_are_uid_scoped`; `wallet_database::repositories::api_business_notification::tests::duplicate_delivery_preserves_read_and_resolution_state` |

MQTT ingestion and fresh-balance resolution are separate stages under the confirmed
backend contract. No external backend or chain calls occur in the tests above.

## Balance MQTT envelope parsing

`AwmAddressInsufficient::from_message` parses the confirmed `chainCode`,
`targetAddress`, `waddressType` and decimal fields. It copies the outer UID into
its serializable task body; msgId remains the durable queue ID. It rejects
missing UID, empty chain/address/symbol, empty or whitespace msgId, and a
non-API-withdrawal wallet type. `address_insufficient::tests` covers field spelling,
numeric/string decimals, replay serialization, invalid identity and msgId.

## Durable task name

The `AwmAddressInsufficient` task name is recognized when read back from SQLite.
The focused `address_insufficient_task_name_survives_sqlite_round_trip` test binds
the known task name and asserts that SQLite decoding preserves the known variant.

## MQTT reminder ingestion

`AWM_ADDRESS_INSUFFICIENT` is routed from the ordinary wallet MQTT topic to a
durable task keyed by the existing msgId. Task execution passes that ID to the
notification repository. New notification primary keys use the original globally unique msgId;
existing mqtt-event-v1 rows retain their ID on replay; the address/chain/token business key is retained for grouping.
`redelivery_preserves_snapshot_read_and_resolved_state` verifies same-event replay
is immutable; `different_msg_ids_keep_identical_bodies_and_uid_scope` verifies
independent events for one object and isolation between UIDs. The notification
table remains idempotent after completed queue tasks are cleaned up.

## Balance-based resolution

The unresolved reminder query refreshes each balance alert using the existing API
wallet chain-balance service. It resolves a row when the fresh balance reaches its
stored `thresholdValue`. Query or parse failures leave the row active. The saved
`remaining` display snapshot is unchanged. The focused
`current_balance_resolves_only_after_successful_threshold_check` test covers query
failure, below-threshold balance, exact-threshold balance, and snapshot retention.

## Unresolved Reminder Pagination Regression

- Layer: component; local SQLite and injected balance responses, no network.
- Entrypoint: `wallet-api/src/service/api_wallet/business_notification.rs` / `unresolved_page_checks_items_that_shift_in_after_resolution`.
- Invariant: all initially unresolved reminders for the UID are checked before the final page and `totalCount` are read. A resolved first-page item cannot leave a newly shifted item unchecked.
- Failure behavior: balance query failure keeps the reminder active (covered by `current_balance_resolves_only_after_successful_threshold_check`).

## Message-center unread count and all-read

- Layer: component; temporary API-wallet SQLite, no external network.
- Entrypoint: `wallet_database::repositories::api_business_notification::tests::unread_count_and_mark_all_read_cover_history_and_preserve_uid_isolation`.
- Unread count includes resolved history and records beyond the first page.
- Mark-all-read applies only to the requested UID and can be repeated safely.
- An unknown UID changes no records. Another UID remains unread.
- Read changes preserve payload, creation time, and resolved/unresolved state;
  active banner reminders remain active after marking all messages read.

## Asset-page banner polling

- Layer: component; local SQLite, injected balances and monotonic clock, no network.
- Entrypoint: `wallet_api::service::api_wallet::business_notification::tests::banners_check_all_balances_and_keep_review_and_read_states_separate`.
- One query returns UID-scoped pending review orders (existing `Init` predicate)
  and unresolved balance reminders as distinct typed groups with count/time/items.
- Orders with an audit decision are excluded even before their status advances.
- Each fresh query checks all unresolved reminders in the notification table,
  with four concurrent calls and a three-second request timeout. The frontend
  owns the polling timer. SDK buffers repeated calls for five seconds from check
  completion, per UID, and reads the latest local banner data during that window.
  A mutex serializes overlapping checks so waiting callers reuse the completed
  check. No per-call total limit or pagination cursor is used for balance IO.
- The final count and items are read after all balance checks finish. The test
  checks twelve records in each poll, then confirms five recoveries produce a
  count of seven in the same response.
- Failed balance checks preserve active reminders; marking read does not dismiss
  them. Threshold equality resolves a checked reminder.
- New reminders record `notifiedAt` in their payload from MQTT receipt, persisted
  through task replay. Group time uses it, falling back to `createdAt` for old rows;
  read and resolution updates do not change this reminder time.
- Resolution requires the checked payload to still match the stored row. The
  SQLite `stale_balance_observation_cannot_resolve_new_alert_payload` test covers
  stale thresholds, cross-UID attempts, and a matching observation.
- `banner_buffer_coalesces_calls_but_expires_and_invalidates_changed_alerts`
  covers overlapping calls, the five-second expiry boundary, UID isolation,
  read-state changes, new rows, and changed thresholds. New or changed payloads
  bypass the buffer; removals/read changes do not require another chain check.
- `banner_query_future_supports_plugin_thread_pool` checks the public banner
  future implements Send, as required by the generated FRB async wrapper.
- `business_all_read_covers_all_pages_without_resolving_or_crossing_uid`
  covers twelve reminders across pages (one resolved history item), idempotent all-read, unchanged active
  state and payloads, cross-UID isolation and invalid UID no-op on stored state.
  Unread count includes history; reading never resolves a banner.

## No-UID public withdrawal-wallet message center (2026-10-06)

These current public contracts supersede UID-only message-center statements above;
internal per-UID repo/service paths remain for ingestion and background work.

| Action | Required invariant | Component test |
| --- | --- | --- |
| Global list/count/read | Active Withdrawal scope, duplicate wallet rows do not duplicate reminders, global ordering precedes pagination; resolved unread history counts | `withdrawal_message_center_tests::global_message_center_filters_orders_pages_and_read_writes_by_active_wallet` |
| ID read and all-read | Recheck eligibility in SQL, exclude subaccounts/disabled/orphan/blank owner; repeated reads preserve payload and resolution | same SQLite test |
| Scope-table failure | Return errors, never mark out-of-scope or unread records read on failure | `missing_wallet_scope_table_fails_without_marking_any_message_read` |
| Public no-UID API | Preserve record ownership, no balance lock/chain IO on ordinary list/count/read; invalid page/ID rejected | `no_uid_message_api_keeps_owner_and_reads_without_balance_checks` |
| Existing unresolved refresh | Filter eligible scope before balance IO, resolve exact threshold with payload CAS, failed checks retain messages, repage after resolution | `global_unresolved_refresh_filters_before_chain_io_and_repages_after_resolution` |

No scale benchmark, App integration or live chain verification. Count/page are
separate existing SQL reads; concurrent mutations may change offset pagination.
Direct unresolved refresh retains serial balance IO and has no added total deadline;
background directed checks and pure local balance query remain separate.

## Lightweight presentation and local detail (2026-10-07)

| Boundary | Evidence |
| --- | --- |
| Summary exact id/kind/isRead/remaining/symbol/notifiedAt, no payload | projection_tests::summary_is_minimal_and_uses_notification_time |
| notifiedAt differs from first insert/update; insertion-time fallback only if absent | projection_tests timestamp tests |
| Typed detail coin/balance/merchant/address/chain/type; unknown/malformed rejected | projection_tests detail/error tests |
| DAO active withdrawal scope, duplicate wallets, excluded owners, SQL id binding, wallet disable | repositories::api_business_notification::detail_tests |
| Resolved history detail/list readable; banner unresolved; read does not mark read/resolve/RPC | service::api_wallet::business_notification::detail_tests |

No new withdrawal notification kind is persisted by this flow. Withdrawal order amount/tradeNo/detail/audit interfaces remain separate. Unknown kinds retain summary metadata with null balance fields, not an invented amount projection.

Kind typing: reuse BizType rather than introducing a second enum; mapping only admits currently persisted AWM_ADDRESS_INSUFFICIENT. Enum serialization/deserialization and unsupported event/numeric/case/blank strings are covered by projection_tests::existing_business_enum_preserves_wire_string_and_rejects_non_notification_kinds. Existing unknown-summary metadata and detail422 tests remain authoritative.

## Event history candidate

| Invariant | Component test |
| --- | --- |
| Different explicit IDs preserve same-object history, per-event read and resolution | database `different_events_same_object_keep_history_and_single_read_isolation` |
| Same event ID cannot change payload/time/read/resolution | database `event_id_replay_never_reopens_or_mutates_event_snapshot` |
| Immutable event time, nanoseconds, and ID tie-breaking precede offset pagination | database `event_time_precedes_insertion_order_and_id_breaks_ties_across_pages`; `submillisecond_event_time_is_not_replaced_by_id_order` |
| Initial notification schema permits same-object events, nanosecond time format and four target indexes without upgrade migration | database `initial_notification_schema_supports_event_history_without_upgrade` |
| Banner grouping precedes limit/count; valid wallets only; malformed history remains readable as generic history | service `banner_aggregates_before_pagination_and_keeps_latest_valid_unresolved_event` |
| Latest valid event controls the whole group: higher threshold blocks older low events; lower threshold resolves all observed older events atomically, without read/history changes | service `latest_higher_threshold_blocks_older_lower_event_without_reading_or_hiding_history`; `latest_lower_threshold_resolves_older_high_events_and_preserves_history_read_state`; `persisted_balance_uses_latest_higher_threshold_for_whole_group` |
| New arrivals cannot inherit an in-flight or previously cached observation | service `in_flight_balance_observation_does_not_resolve_new_event`; `cached_observation_cannot_resolve_event_arriving_after_original_check`; `asset_sync_observation_cannot_resolve_event_arriving_during_rpc` |

All fixtures are isolated temporary SQLite and injected balance closures. No real
backend, chain, App, or user database is exercised. Production MQTT reuses Message.msg_id via the durable task ID, used directly for new notification IDs, with legacy-ID lookup on replay; empty IDs return observable parameter errors.
MQTT regressions cover different messages with identical bodies, retransmission state protection, UID scope, and per-event states. Existing page/pageSize
API remains; concurrent inserts may shift offset boundaries.

Database scope: development fresh install/recreation only. The initial notification migration is changed by explicit user authorization; existing databases with the previous checksum are not supported by this candidate. No compatibility copy/rebuild migration remains. Other migrations are unchanged.

## Latest-threshold rule (product confirmation 2026-10-07; supersedes per-event thresholds)

Group identity: uid/kind/chain/address/token. Latest valid event by created_at DESC/id DESC controls the threshold; selection includes resolved history, so a resolved latest event never exposes older unresolved rows. History/read/msgId/public signatures/schema unchanged.

Directed tasks for superseded events exit without chain IO. Ready seed selects latest unresolved representatives. A fresh latest check captures the group before RPC; a buffered retry uses that exact original snapshot and timestamp. Asset sync captures all eligible history before RPC and groups by the same rule. A successful observation resolves only the selected latest and observed earlier members, in one transaction with id/uid/payload/time/resolution guards. Later arrivals and changed versions are excluded; SQL failures roll back the whole group. is_read is never written by resolution.

Key evidence: resolved_latest_never_exposes_or_checks_older_unresolved_event; old_directed_task_uses_latest_threshold_and_tie_breaks_with_uid_isolation; group_resolution_write_failure_rolls_back_every_event; observed_group_versions_and_uid_are_guarded; observed_group_resolution_is_atomic_and_preserves_read_state; asset_sync_latest_lower_threshold_resolves_observed_older_high_event. RPC/cache/asset arrival regressions use lower new thresholds than returned balances. Full MQTT task restart/real backend ACK and large-history performance remain unverified.

Writer-lock regression `observed_group_waits_for_other_writer_before_reading_versions` uses a second SQLite connection: default DEFERRED transaction failed RED; BEGIN IMMEDIATE waits under existing busy timeout before version reads. `observed_group_does_not_capture_event_inserted_during_resolution` inserts a low-threshold event during UPDATE via a trigger and verifies it stays unresolved. `group_cached_compensation_does_not_apply_old_observation_to_new_latest` covers multi-event rollback/cache compensation followed by a newer lower threshold requiring fresh RPC.

## WALLET-2946 notification database contention (2026-10-26)

Scope: Context's API-wallet pool wiring and the message-center single-item read
update. Other database wrappers and reminder business semantics are unchanged.

| Invariant | Regression evidence |
| --- | --- |
| Context keeps separate reader/writer pools; a checked-out writer cannot exhaust reader capacity. | `wallet_2946_detail_remains_available_while_writer_connection_is_held` calls public detail during an open writer transaction, then marks read and reads persisted state. |
| Separate Context accessor calls share one writer gate; cancelling a waiting mark-read changes no read state and does not prevent a later update. | `wallet_2946_context_calls_share_the_same_write_gate`; `wallet_2946_mark_read_waits_for_gate_and_cancelled_wait_preserves_unread` |
| A transient external writer conflict uses existing bounded SQLite retries and preserves historical payload, business identity, creation time and resolution state. Duplicate and missing IDs retain no-op behavior. | `wallet_2946_mark_read_retries_external_writer_and_preserves_history` |
| Persistent writer conflict still returns SQLite busy, leaves the alert unread and releases coordination resources; a later call can succeed. | `wallet_2946_exhausted_retry_preserves_unread_and_releases_gate` |

Tests use migrated local SQLite, with no backend/RPC calls. Only the database
component tests shorten the writer connection's busy timeout to exercise retries;
production remains at 5 seconds busy wait and 60 seconds acquire wait. Bounded
retry cannot hide persistent external writers or exhausted reader connections.
The earlier controlled SDK diagnostics established reachable error paths; these
regressions do not identify the production incident's actual lock holder.

## MQTT banner msgId identity (2026-10-28)

| Contract | Offline regression |
| --- | --- |
| New notification IDs equal msgId; distinct globally unique IDs remain UID-scoped. | `different_msg_ids_keep_identical_bodies_and_uid_scope` |
| Preserve exact valid bytes, including non-ASCII, delimiters and surrounding spaces; concurrent replay inserts one row. | `raw_msg_id_keeps_original_bytes_and_concurrent_replays_deduplicate` |
| Legacy v1 replay retains original ID, payload, creation/update times, read and resolution state; selected ID is used by exec's follow-up lookup. | `legacy_redelivery_retains_existing_id_and_all_notification_state`; exec → persist returns selected ID → find/enqueue |
| Reject empty/whitespace IDs before queue/database writes; same raw-ID replay preserves snapshot and state. | `empty_msg_id_rejected_before_queue_and_database`; `redelivery_preserves_snapshot_read_and_resolved_state` |

Concurrency uses existing INSERT ON CONFLICT(id) DO NOTHING. No new v1 rows are
created by this implementation, so concurrent retries select the same existing v1
ID or the same raw ID. A lookup failure propagates before insertion; a failure or
cancellation after insertion retries the same identity. No runtime migration or
historical state rewrite is performed. Mixed old/new SDK writers against one DB,
real backend uniqueness, full task restart/ACK and App display remain unverified.

Backend contract confirmed by the user in this chat: msgId has a fixed format
excluding `/` and `:`, disjoint from historical notification ID namespaces.
The SDK does not add a new format restriction; the byte-preservation test probes
existing parser behavior, not a claim that backend sends arbitrary formats.
## Persisted EVM balance reconciliation contract case (2026-10-29)

| Contract | Offline regression |
| --- | --- |
| A mixed-case Avalanche notification contract matches a persisted lowercase contract, and vice versa; threshold-reaching balance resolves the row without changing payload/read state. | `synced_balance_matches_evm_contract_case_without_merging_other_assets` |
| Different contracts and case-different Tron identifiers do not resolve another asset's reminder. | `synced_balance_matches_evm_contract_case_without_merging_other_assets` |
| Preserve UID/address/chain/native isolation and buffer invalidation; insufficient, invalid and absent persisted balances stay unresolved. | `synced_balance_removes_only_matching_reminder_inside_balance_query_buffer`; `synced_balance_keeps_reminder_for_insufficient_invalid_or_unpersisted_balance` |

The regression enters through the active withdrawal-wallet pre-RPC snapshot,
then reconciles persisted balances and checks the actual unresolved-banner query.
Reuse chain-aware token normalization at both snapshot selection and persisted
observation lookup; event history payload (including remaining) is not rewritten.
Snapshot, locks, conditional resolution and retry behavior remain unchanged;
no chain RPC or device database mutation is part of these offline tests.

## API wallet notification invalidation and unread naming (2026-10-30)

| Boundary | Required behavior | Regression |
| --- | --- | --- |
| MQTT publication | Persist before API_WALLET_NOTIFICATION_CHANGED, data is empty object; publish before optional balance-check dispatch can fail | notification_event_is_published_after_persistence_and_replay_preserves_read_state |
| Failed storage | No refresh event on failed insert | failed_notification_persistence_emits_no_event |
| Unavailable frontend | Absent/closed sender does not stop persistence or directed task enqueue | unavailable_frontend_does_not_prevent_balance_check_enqueue |
| Replay | Repeated invalidation allowed; payload/time/read facts unchanged | notification_event_is_published_after_persistence_and_replay_preserves_read_state |
| Unread API | count_unread_api_wallet_notifications includes unknown kinds/resolved unread history, excludes read/sub/disabled/orphan rows; no balance RPC | api_wallet_unread_count_includes_future_kinds_and_excludes_ineligible_owners; no_uid_message_api_keeps_owner_and_reads_without_balance_checks |

Old public count_unread_api_business_notifications is removed. Plugin must be
regenerated against the reviewed committed SDK revision before App migration.
Events are transient invalidations, not ordered snapshots or durable delivery.
Existing read callers refresh after success; no new resolution/read event contract
is added in this batch. App must prevent stale query responses overwriting new
state and reload on entry/foreground. No App/device validation is claimed here.


## WALLET-2979 resolution recovery (2026-10-30)

| Contract | Offline regression |
| --- | --- |
| Failed notification snapshot returns before RPC; dirty target remains recoverable, successful retry resolves history | notification_recovery_snapshot_failure_retains_dirty_target_before_rpc |
| A genuine empty snapshot succeeds; 1,000 unrelated stored assets cause no additional balance calls or writes | notification_recovery_empty_snapshot_is_success_without_scope_expansion |
| Resolution plus compensation storage failure keeps only the failed asset dirty; successful sibling completes and publishes its transition | notification_recovery_failed_compensation_keeps_only_affected_asset_dirty |
| Persisted directed compensation permits completion even without live dispatcher; recent successful observation is reused | notification_recovery_durable_compensation_allows_asset_completion_without_dispatch; notification_recovery_enqueue_success_means_durable_even_without_dispatcher |
| Multiple committed resolutions coalesce, duplicates emit nothing and preserve unread/history | notification_recovery_persisted_resolution_emits_once_after_commit_and_not_on_replay |
| Directed check emits only for actual threshold-reaching resolution; insufficient balance adds no retry/poll | notification_recovery_directed_checks_publish_only_actual_resolution |
| Closed frontend retains committed state and creates no compensation/sync work | notification_recovery_closed_frontend_keeps_resolution_committed_without_compensation |
| Old pre-RPC snapshot cannot clear a later alert | notification_recovery_old_snapshot_does_not_resolve_new_alert_or_publish; newer_event_arriving_during_check_cannot_be_resolved_by_old_result |

This batch extends the earlier notification invalidation contract to resolutions.
Normal success reuses persisted balance observations; only failed notification
work uses existing directed tasks/dirty asset recovery. No full-wallet polling,
RPC concurrency increase or schema change. Small scoped fixtures establish target
selection, not phone scheduling or 500k throughput; App acceptance is unverified.

Validation checkpoint: corrected RED 4 pass/5 expected failures; GREEN regressions
9/9, business_notification 51/51, api_wallet_balance_check 5/5. Logs are under
target/wallet-2979. Test profile overrides apply only to the command (wallet-api
debug=0, codegen-units=64); repository profiles are unchanged. Final receipt
compatibility evidence and fixed-version independent review are recorded with
the delivery snapshot under target/wallet-2979/review.

Final compatibility: notification-current9/9 and receipt_refresh4/4 passed. The
receipt command uses default features plus tokio/io-std to satisfy the repository's
auto-built console worker; minimal/plain default commands fail before test execution.
No manifest changes. Independent read-only source review found no actionable
findings; final docs/evidence supplement is stored with the fixed review snapshot.

### Import balance reconciliation (WALLET-2979 follow-up, 2026-11-02)
Component regressions: `infrastructure::api_wallet_assets_sync::import_notification_tests`. Missing local account/handles: imported balance resolves without extra RPC and replay emits no event; insufficient balance and new event during observation stay unresolved; notification snapshot failure stops before backend call; subaccount import bypasses alert table; resolution failure persists directed task without dispatcher; compensation persistence failure retains only the affected asset; latest resolved event suppresses historical alerts and 1000 unrelated asset balances stay untouched. Existing query/sync priority and request ranges remain unchanged. Real 500k/device timing and crash injection remain uncovered.
