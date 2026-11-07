# Bill creation

Scope: `BillRepo::create` / `BillDao::create`, including the existing upsert.

| Scenario | Required result | Regression |
| --- | --- | --- |
| Ordinary bill | Persist and retrieve the bill | `bill_repo_create_and_get_by_hash_opt_success` |
| Text fields contain apostrophes | Preserve the exact address and note text as data | `bill_repo_create_preserves_apostrophes_in_text_fields` |
| Transaction rolls back | Bill remains absent | `bill_repo_tx_rollback_keeps_bill_absent` |
| Owner contains an apostrophe | Latest matching bill is returned | `bill_repo_last_kind_bill_accepts_apostrophe_in_owner` |
| Owner and hash contain apostrophes | Matching hash list is returned | `bill_repo_lists_by_hashs_accepts_apostrophes` |
| Paginated text filters contain apostrophes | Matching data and total count are returned | `bill_repo_bill_lists_binds_text_filters_and_counts_matches` |
| Recent recipient filters contain apostrophes | Matching unique recipient is returned | `bill_dao_recent_bill_binds_text_filters` |

This coverage includes bill insertion, two non-pagination lookups, and the main
bill-list pagination and recent-recipient queries. Other DAOs remain outside its
scope.


## Decimal bill amounts (2026-10-13)

Scope: ordinary-wallet bill amount insertion and list/detail display; no schema
migration, clearing, or rewriting existing records.

| Input / invariant | Expected behavior | Regression |
| --- | --- | --- |
| Small/large/negative/zero new amount; >8 fractional digits | Exact f64 decimal text stored as TEXT, no exponent and no display truncation at write | `bill_new_amounts_are_stored_as_plain_text_without_truncation` |
| Historical exponent, uppercase E, negative exponent, integer exponent, tiny amount | Exact string expansion then existing 8dp truncation; ordinary decimal behavior retained | `bill_amount_displays_legacy_exponents_as_plain_decimals` |
| Existing nonzero exponent record read and duplicate upsert | Plain display; stored historical amount unchanged; no duplicate row | `bill_legacy_amounts_display_without_rewriting_history_or_upsert_value` |
| Existing zero record receives actual amount | Conflict update fills plain decimal amount, retains one row | `bill_legacy_amounts_display_without_rewriting_history_or_upsert_value` |
| NaN or positive/negative infinity | Error before write; no new rows | `bill_nonfinite_amounts_fail_without_writing` |

Evidence: `cargo test --offline --target-dir target -p wallet-database --lib bill`
passed 15 tests (4 new regressions + 11 existing), exit 0. New regressions first
failed against old behavior. Existing rollback, text filters, pagination and
SQL binding regressions passed unchanged. List/detail call the same entity display
method; an Android rebuild and UI verification are not covered. Incoming amount
precision remains f64; this change cannot recover precision already lost before
persistence.
