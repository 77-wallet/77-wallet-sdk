# Ordinary-wallet coin precision synchronization

Scope: core `coin` and `assets` tables only, no schema change or live network.

| Entry / risk | Required assertion | Offline component test |
| --- | --- | --- |
| Price / MQTT precision 9 -> 18 | All owners for the same chain/token receive new precision; cached integer-unit balance preserved | `precision_update_repairs_existing_balances_and_is_idempotent` |
| Startup coin upsert, coin already 18 | Assets still at 9 are repaired | `precision_upsert_repairs_assets_even_if_coin_is_already_correct` |
| Chain / contract isolation | Same owner on another chain and contract on the same chain remain unchanged | `precision_update_repairs_existing_balances_and_is_idempotent` |
| Repeated delivery | Corrected balance is not scaled twice | `precision_update_repairs_existing_balances_and_is_idempotent` |
| Malformed cached amount | Whole transaction rolls back: coin price, precision and every asset unchanged | `precision_invalid_cached_balance_rolls_back_coin_and_assets` |
| Price update without precision | No asset mutation | `precision_price_only_update_preserves_assets` |
| Zero decimals | Integer trailing zeroes and zero balance preserved | `precision_zero_decimals_preserves_integer_trailing_zeroes` |
| Multiple query/update batches | 450 tokens / 900 assets corrected; duplicate metadata deduplicated; other tokens untouched | `precision_batch_handles_many_tokens_and_owners_without_touching_other_tokens` |
| Failure beyond one update batch | Every asset and coin upsert rolled back | `precision_batch_failure_rolls_back_all_asset_batches_and_coin_upsert` |
| Manual offline scale | 1,000 tokens / 20,000 assets correct after initial repair and two repeated deliveries; no timing threshold | `precision_scale_1000_tokens_20000_assets` (`--ignored --nocapture`) |

Cached balance correction is not a new on-chain query. Existing balance refresh
uses the repaired asset precision on its next fetch. Transaction/bill history is
not rewritten. API-wallet precision synchronization remains outside this batch.
