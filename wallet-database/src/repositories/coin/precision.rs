use crate::entities::asset_token_key::AssetTokenKey;

/// Repair the duplicated precision and cached display amount atomically with
/// coin metadata. Preserve integer units; never rescale through floating point.
pub(super) async fn sync_asset_precision(
    conn: &mut sqlx::SqliteConnection,
    chain_code: &str,
    token_key: &AssetTokenKey,
    decimals: u8,
) -> Result<(), crate::Error> {
    sync_asset_precision_batch(conn, &[(chain_code.to_owned(), token_key.clone(), decimals)]).await
}

// Three binds per row, plus one timestamp on UPDATE: stay below even SQLite's
// older 999-variable limit. Bound both query size and each UPDATE batch.
const BATCH_SIZE: usize = 200;

pub(super) async fn sync_asset_precision_batch(
    conn: &mut sqlx::SqliteConnection,
    keys: &[(String, AssetTokenKey, u8)],
) -> Result<(), crate::Error> {
    let normalized: Vec<_> = keys
        .iter()
        .map(|(chain, token, decimals)| {
            (chain.clone(), token.normalized_for_chain(chain), *decimals)
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    let keys: Vec<_> = normalized
        .iter()
        .filter(|(chain, token, decimals)| {
            seen.insert((chain.as_str(), token.as_db_str(), *decimals))
        })
        .collect();
    for keys in keys.chunks(BATCH_SIZE) {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "WITH changed(chain_code, token_address, decimals) AS (",
        );
        query.push_values(keys, |mut row, (chain, token, decimals)| {
            row.push_bind(chain).push_bind(token.as_db_str()).push_bind(decimals);
        });
        query.push(
            ") SELECT a.rowid, a.decimals, k.decimals, a.balance
             FROM assets a JOIN changed k ON a.chain_code = k.chain_code
             AND COALESCE(a.token_address, '') = k.token_address
             WHERE a.decimals != k.decimals
             AND EXISTS (SELECT 1 FROM coin c WHERE c.chain_code = k.chain_code
             AND c.token_address = k.token_address AND c.decimals = k.decimals)",
        );
        let rows: Vec<(i64, u8, u8, Option<String>)> = query
            .build_query_as()
            .fetch_all(&mut *conn)
            .await
            .map_err(crate::DatabaseError::from)?;
        for rows in rows.chunks(BATCH_SIZE) {
            let mut corrected = Vec::with_capacity(rows.len());
            for (row_id, old_decimals, decimals, balance) in rows {
                let balance = balance
                    .as_deref()
                    .map(|balance| {
                        let raw = wallet_utils::unit::convert_to_u256(balance, *old_decimals)?;
                        wallet_utils::unit::format_to_string(raw, *decimals)
                    })
                    .transpose()?;
                corrected.push((*row_id, *decimals, balance));
            }
            let mut update = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "WITH repaired(row_id, decimals, balance) AS (",
            );
            update.push_values(&corrected, |mut row, (row_id, decimals, balance)| {
                row.push_bind(row_id).push_bind(decimals).push_bind(balance);
            });
            update.push(") UPDATE assets SET decimals = repaired.decimals, balance = repaired.balance, updated_at = ")
                .push_bind(chrono::Utc::now().to_rfc3339())
                .push(" FROM repaired WHERE assets.rowid = repaired.row_id");
            update.build().execute(&mut *conn).await.map_err(crate::DatabaseError::from)?;
        }
    }
    Ok(())
}
