//! Durable replacement ownership and verified legacy-counter recovery.
use crate::{
    ApiTransactionDbPool,
    dao::{api_collect::ApiCollectDao, api_nonce::ApiNonceDao},
    entities::api_collect::{ApiCollectStatus, ErrCode},
};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct CollectReplacementScope {
    pub chain_code: String,
    pub from_addr: String,
    pub old_trade_no: String,
    pub old_hash: String,
    pub nonce: i64,
    pub legacy_counter: i64,
}
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CollectReplacement {
    pub new_trade_no: String,
    pub old_trade_no: String,
    pub chain_code: String,
    pub from_addr: String,
    pub nonce: i64,
    pub old_hash: String,
    pub old_raw: String,
    pub legacy_counter: i64,
    pub new_hash: Option<String>,
    pub new_raw: Option<String>,
    pub winner_hash: Option<String>,
    pub abandoned_at: Option<String>,
    pub execution_success: Option<bool>,
    pub actual_transaction_fee: Option<String>,
    pub actual_resource_consume: Option<String>,
    pub block_hash: Option<String>,
    pub block_number: Option<i64>,
    pub transaction_time: Option<String>,
}
pub struct ReplacementResolution<'a> {
    pub transaction_fee: &'a str,
    pub resource_consume: &'a str,
    pub winner_hash: &'a str,
    pub success: bool,
    pub block_hash: &'a str,
    pub block_number: i64,
    pub transaction_time: &'a str,
}
pub struct CollectReplacementRepo;
impl CollectReplacementRepo {
    pub async fn pending_orders(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<crate::entities::api_collect::ApiCollectEntity>, crate::Error> {
        sqlx::query_as("SELECT c.* FROM api_collect c JOIN api_collect_replacement r ON r.new_trade_no=c.trade_no WHERE r.winner_hash IS NULL AND r.abandoned_at IS NULL AND r.new_hash IS NOT NULL ORDER BY r.created_at LIMIT ?").bind(limit as i64).fetch_all(pool.read_ref()).await.map_err(|e|crate::Error::Database(e.into()))
    }
    pub async fn find(
        pool: &ApiTransactionDbPool,
        new_trade: &str,
    ) -> Result<Option<CollectReplacement>, crate::Error> {
        sqlx::query_as("SELECT * FROM api_collect_replacement WHERE new_trade_no=?")
            .bind(new_trade)
            .fetch_optional(pool.read_ref())
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
    /// Discover one already-reported failed intent; observation does not reserve its slot.
    /// The worker checks the confirmed frontier and prepare revalidates shared ownership.
    pub async fn discover(
        pool: &ApiTransactionDbPool,
        new_trade: &str,
    ) -> Result<Option<CollectReplacementScope>, crate::Error> {
        let new = ApiCollectDao::get_api_collect_by_trade_no(pool.read_ref(), new_trade).await?;
        if new.token_addr.is_contract()
            || new.raw_tx.is_some()
            || new.tx_hash.is_some()
            || new.err_code.is_some()
            || new.is_fact_terminal()
            || new.transaction_time.is_some()
            || new.last_broadcast_at.is_some()
            || new.broadcast_uncertain_since_at.is_some()
            || new.tx_exec_receipt_uploaded_at.is_some()
        {
            return Ok(None);
        }
        let prefix = crate::entities::api_nonce_claim::COLLECT_NONCE_GAP_REPORTED;
        let candidates: Vec<crate::entities::api_collect::ApiCollectEntity> = sqlx::query_as(
            "SELECT old.* FROM api_collect old WHERE old.uid=? AND old.chain_code=?
             AND lower(old.from_addr)=lower(?) AND lower(trim(old.to_addr))=lower(trim(?))
             AND old.trade_no<>? AND old.tx_hash IS NOT NULL AND old.raw_tx IS NOT NULL
             AND old.err_code IS NOT NULL AND substr(old.err_msg,1,length(?))=?
             AND old.transaction_time IS NULL AND old.tx_exec_receipt_uploaded_at IS NOT NULL
             AND NOT EXISTS(SELECT 1 FROM api_collect_replacement r WHERE r.old_trade_no=old.trade_no
               AND r.old_hash=old.tx_hash AND r.winner_hash IS NOT NULL AND r.block_hash IS NOT NULL AND r.block_number>0)
             ORDER BY old.nonce LIMIT 2"
        ).bind(&new.uid).bind(&new.chain_code).bind(&new.from_addr).bind(&new.to_addr)
            .bind(new_trade).bind(prefix).bind(prefix).fetch_all(pool.read_ref()).await
            .map_err(|e| crate::Error::Database(e.into()))?;
        // Never choose arbitrarily between historical signed intents.
        if candidates.len() != 1 {
            return Ok(None);
        }
        let old = &candidates[0];
        if old.token_addr != new.token_addr {
            return Ok(None);
        }
        let counter: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(nonce),0) FROM api_nonce WHERE lower(from_addr)=lower(?) AND chain_code=?")
            .bind(&new.from_addr).bind(&new.chain_code).fetch_one(pool.read_ref()).await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(Some(CollectReplacementScope {
            chain_code: new.chain_code,
            from_addr: new.from_addr,
            old_trade_no: old.trade_no.clone(),
            old_hash: old.tx_hash.clone().unwrap(),
            nonce: i64::from(old.nonce),
            legacy_counter: counter.max(i64::from(old.nonce)),
        }))
    }
    /// A retained authorization stops selecting the old slot only after verified resolution.
    pub async fn scope_is_resolved(
        pool: &ApiTransactionDbPool,
        scope: &CollectReplacementScope,
    ) -> Result<bool, crate::Error> {
        let resolved:i64=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM api_collect_replacement WHERE old_trade_no=? AND old_hash=? AND chain_code=? AND from_addr=lower(trim(?)) AND nonce=? AND legacy_counter=? AND abandoned_at IS NULL AND winner_hash IS NOT NULL AND block_hash IS NOT NULL AND block_number>0)")
            .bind(&scope.old_trade_no).bind(&scope.old_hash).bind(&scope.chain_code).bind(&scope.from_addr).bind(scope.nonce).bind(scope.legacy_counter).fetch_one(pool.read_ref()).await.map_err(|e|crate::Error::Database(e.into()))?;
        Ok(resolved != 0)
    }
    /// Capture a discovered intent after atomic ownership validation.
    pub async fn prepare(
        pool: &ApiTransactionDbPool,
        new_trade: &str,
        scope: &CollectReplacementScope,
        chain_next: i64,
    ) -> Result<Option<CollectReplacement>, crate::Error> {
        if scope.chain_code.is_empty()
            || chain_next < scope.nonce
            || scope.nonce < 0
            || scope.legacy_counter < scope.nonce
            || new_trade == scope.old_trade_no
        {
            return Ok(None);
        }
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        sqlx::query("UPDATE api_collect SET nonce=nonce WHERE trade_no=?")
            .bind(new_trade)
            .execute(&mut *tx)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        let new = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, new_trade).await?;
        let old = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, &scope.old_trade_no).await?;
        if new.from_addr != new.from_addr.trim()
            || old.from_addr != old.from_addr.trim()
            || scope.from_addr != scope.from_addr.trim()
            || new.chain_code != scope.chain_code
            || old.chain_code != scope.chain_code
            || !new.from_addr.trim().eq_ignore_ascii_case(&scope.from_addr)
            || !old.from_addr.trim().eq_ignore_ascii_case(&scope.from_addr)
            || old.nonce as i64 != scope.nonce
            || old.tx_hash.as_deref() != Some(scope.old_hash.as_str())
            || old.raw_tx.is_none()
            || !old.err_msg.as_deref().is_some_and(|s| {
                s.starts_with(crate::entities::api_nonce_claim::COLLECT_NONCE_GAP_REPORTED)
            })
            || old.err_code.is_none()
            || old.transaction_time.is_some()
            || new.raw_tx.is_some()
            || new.tx_hash.is_some()
            || new.last_broadcast_at.is_some()
            || new.broadcast_uncertain_since_at.is_some()
            || new.tx_exec_receipt_uploaded_at.is_some()
            || new.is_fact_terminal()
            || new.transaction_time.is_some()
            || new.err_code.is_some()
            || new.token_addr.is_contract()
            || new.token_addr != old.token_addr
            || !new.to_addr.trim().eq_ignore_ascii_case(old.to_addr.trim())
        {
            return Ok(None);
        }
        // Existing shared ownership queries use case folding, not whitespace aliases.
        // Refuse an ambiguous legacy identity rather than silently miss its claims.
        let padded_alias: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM (SELECT from_addr,chain_code FROM api_collect UNION ALL SELECT from_addr,chain_code FROM api_fee UNION ALL SELECT from_addr,chain_code FROM api_withdraws UNION ALL SELECT from_addr,chain_code FROM api_nonce) WHERE chain_code=? AND lower(trim(from_addr))=lower(?) AND from_addr<>trim(from_addr))")
            .bind(&scope.chain_code).bind(&scope.from_addr).fetch_one(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        if padded_alias != 0 {
            return Ok(None);
        }
        let owners = ApiNonceDao::nonce_claims(
            &mut *tx,
            &old.from_addr,
            &old.chain_code,
            scope.nonce,
            scope.nonce,
        )
        .await?;
        if owners.len() != 1 || owners[0].tx_hash.as_deref() != Some(scope.old_hash.as_str()) {
            return Ok(None);
        }
        let counter: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(nonce),0) FROM api_nonce WHERE lower(from_addr)=lower(?) AND chain_code=?")
            .bind(&old.from_addr).bind(&old.chain_code).fetch_one(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        if counter > scope.legacy_counter {
            return Ok(None);
        }
        sqlx::query("INSERT INTO api_collect_replacement(new_trade_no,old_trade_no,chain_code,from_addr,nonce,old_hash,old_raw,legacy_counter) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(new_trade).bind(&old.trade_no).bind(&old.chain_code).bind(old.from_addr.trim().to_lowercase()).bind(scope.nonce).bind(&scope.old_hash).bind(old.raw_tx.unwrap()).bind(scope.legacy_counter).execute(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        let relation:Option<CollectReplacement>=sqlx::query_as("SELECT * FROM api_collect_replacement WHERE new_trade_no=? AND old_trade_no=? AND winner_hash IS NULL AND abandoned_at IS NULL").bind(new_trade).bind(&old.trade_no).fetch_optional(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(relation)
    }
    /// Rejected quote is a business failure, never permission to erase either signature.
    pub async fn fail_signed_quote(
        pool: &ApiTransactionDbPool,
        trade: &str,
    ) -> Result<bool, crate::Error> {
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        let result=sqlx::query("UPDATE api_collect SET err_code=?,err_msg='replacement_requires_requote: node rejected replacement price; both signed identities retained, no automatic re-sign',updated_at=CURRENT_TIMESTAMP WHERE trade_no=? AND transaction_time IS NULL AND err_code IS NULL AND EXISTS(SELECT 1 FROM api_collect_replacement r WHERE r.new_trade_no=api_collect.trade_no AND r.new_hash IS NOT NULL AND r.winner_hash IS NULL AND r.abandoned_at IS NULL)")
            .bind(ErrCode::TransactionOnChainException as i64).bind(trade).execute(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        let changed = result.rows_affected() > 0;
        if changed {
            let row = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, trade).await?;
            ApiCollectDao::update_status(&mut *tx, trade, row.recompute_status()).await?;
        }
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(changed)
    }
    pub async fn fail_unsigned_attempt(
        pool: &ApiTransactionDbPool,
        trade: &str,
        message: &str,
    ) -> Result<bool, crate::Error> {
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        let count=sqlx::query("UPDATE api_collect_replacement SET abandoned_at=CURRENT_TIMESTAMP WHERE new_trade_no=? AND new_hash IS NULL AND winner_hash IS NULL AND abandoned_at IS NULL AND EXISTS(SELECT 1 FROM api_collect c WHERE c.trade_no=new_trade_no AND c.raw_tx IS NULL AND c.tx_hash IS NULL AND c.transaction_time IS NULL)").bind(trade).execute(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?.rows_affected();
        if count > 0 {
            ApiCollectDao::update_status_and_err(
                &mut *tx,
                trade,
                ApiCollectStatus::SendingTxFailed,
                ErrCode::FeeInsufficient,
                message,
            )
            .await?;
        }
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(count > 0)
    }
    /// Called inside the signed-collect writer. Does not allow fee/withdraw to reuse this slot.
    pub(crate) async fn permits_signed(
        conn: &mut sqlx::SqliteConnection,
        trade: &str,
        nonce: i64,
        chain_next: i64,
    ) -> Result<bool, crate::Error> {
        let r:Option<CollectReplacement>=sqlx::query_as("SELECT * FROM api_collect_replacement WHERE new_trade_no=? AND nonce=? AND winner_hash IS NULL AND abandoned_at IS NULL AND new_hash IS NULL").bind(trade).bind(nonce).fetch_optional(&mut *conn).await.map_err(|e|crate::Error::Database(e.into()))?;
        let Some(r) = r else { return Ok(false) };
        if chain_next != nonce {
            return Ok(false);
        }
        let old = ApiCollectDao::get_api_collect_by_trade_no(&mut *conn, &r.old_trade_no).await?;
        if old.tx_hash.as_deref() != Some(r.old_hash.as_str())
            || old.raw_tx.as_deref() != Some(r.old_raw.as_str())
            || old.transaction_time.is_some()
        {
            return Ok(false);
        }
        let new = ApiCollectDao::get_api_collect_by_trade_no(&mut *conn, trade).await?;
        if new.chain_code != r.chain_code
            || !new.from_addr.trim().eq_ignore_ascii_case(&r.from_addr)
            || new.token_addr != old.token_addr
            || !new.to_addr.trim().eq_ignore_ascii_case(old.to_addr.trim())
        {
            return Ok(false);
        }
        let counter: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(nonce),0) FROM api_nonce WHERE lower(from_addr)=lower(?) AND chain_code=?")
            .bind(&old.from_addr).bind(&old.chain_code).fetch_one(&mut *conn).await.map_err(|e|crate::Error::Database(e.into()))?;
        if counter > r.legacy_counter {
            return Ok(false);
        }
        let owners =
            ApiNonceDao::nonce_claims(&mut *conn, &r.from_addr, &r.chain_code, nonce, nonce)
                .await?;
        Ok(owners.len() == 1 && owners[0].tx_hash.as_deref() == Some(r.old_hash.as_str()))
    }
    pub(crate) async fn record_signed(
        conn: &mut sqlx::SqliteConnection,
        trade: &str,
        hash: &str,
        raw: &str,
    ) -> Result<(), crate::Error> {
        let identity:Option<(String,String)>=sqlx::query_as("SELECT old_hash,old_raw FROM api_collect_replacement WHERE new_trade_no=? AND abandoned_at IS NULL AND winner_hash IS NULL").bind(trade).fetch_optional(&mut *conn).await.map_err(|e|crate::Error::Database(e.into()))?;
        let Some((old_hash, old_raw)) = identity else { return Ok(()) };
        if old_hash == hash || old_raw == raw {
            return Err(crate::Error::Other(
                "replacement must have a distinct signed identity".into(),
            ));
        }
        let rows=sqlx::query("UPDATE api_collect_replacement SET new_hash=?,new_raw=?,updated_at=CURRENT_TIMESTAMP WHERE new_trade_no=? AND new_hash IS NULL AND winner_hash IS NULL AND abandoned_at IS NULL").bind(hash).bind(raw).bind(trade).execute(conn).await.map_err(|e|crate::Error::Database(e.into()))?.rows_affected();
        if rows != 1 {
            return Err(crate::Error::Other("replacement signed association conflict".into()));
        }
        Ok(())
    }
    /// Caller owns canonical chain verification. A pending nonce or null receipt is not evidence.
    pub async fn resolve(
        pool: &ApiTransactionDbPool,
        trade: &str,
        evidence: &ReplacementResolution<'_>,
    ) -> Result<bool, crate::Error> {
        if evidence.block_number <= 0
            || evidence.block_hash.len() != 66
            || evidence.transaction_time.is_empty()
        {
            return Err(crate::Error::Other("invalid replacement chain evidence".into()));
        }
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        sqlx::query(
            "UPDATE api_collect_replacement SET updated_at=updated_at WHERE new_trade_no=?",
        )
        .bind(trade)
        .execute(&mut *tx)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        let r: CollectReplacement =
            sqlx::query_as("SELECT * FROM api_collect_replacement WHERE new_trade_no=?")
                .bind(trade)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;
        let old = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, &r.old_trade_no).await?;
        let new = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, trade).await?;
        let identity_matches = old.chain_code == r.chain_code
            && new.chain_code == r.chain_code
            && old.from_addr.trim().eq_ignore_ascii_case(&r.from_addr)
            && new.from_addr.trim().eq_ignore_ascii_case(&r.from_addr)
            && old.nonce as i64 == r.nonce
            && old.tx_hash.as_deref() == Some(r.old_hash.as_str())
            && old.raw_tx.as_deref() == Some(r.old_raw.as_str())
            && new.tx_hash == r.new_hash
            && new.raw_tx == r.new_raw
            && (r.new_hash.is_none() || new.nonce as i64 == r.nonce);
        if !identity_matches {
            return Err(crate::Error::Other(
                "replacement identities changed before reconciliation".into(),
            ));
        }
        if let Some(w) = r.winner_hash {
            return if w == evidence.winner_hash {
                Ok(false)
            } else {
                Err(crate::Error::Other("replacement winner conflict".into()))
            };
        }
        let linked_new = r.new_hash.as_deref() == Some(evidence.winner_hash);
        let linked_old = evidence.winner_hash == r.old_hash;
        if !linked_new
            && !linked_old
            && !(evidence.winner_hash.len() == 66
                && evidence.winner_hash.starts_with("0x")
                && evidence.winner_hash[2..].bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(crate::Error::Other("invalid canonical nonce-conflict identity".into()));
        }
        if !linked_new || !evidence.success {
            let new = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, trade).await?;
            if new.transaction_time.is_some() {
                return Err(crate::Error::Other(
                    "replacement result conflicts with terminal new order".into(),
                ));
            }
            let message = format!(
                "collect_replacement_result: winner_hash={} old_hash={} new_hash={} execution_success={}; no automatic second transfer",
                evidence.winner_hash,
                r.old_hash,
                r.new_hash.as_deref().unwrap_or("unsigned"),
                evidence.success
            );
            if new.finished_at.is_none() {
                ApiCollectDao::update_status_and_err(
                    &mut *tx,
                    trade,
                    ApiCollectStatus::SendingTxFailed,
                    ErrCode::TransactionOnChainException,
                    &message,
                )
                .await?;
            }
        }
        if r.new_hash.as_deref() == Some(evidence.winner_hash)
            && evidence.success
            && new.err_code.is_none()
            && new.finished_at.is_none()
        {
            ApiCollectDao::confirm_onchain_transaction_fact_with_recover(
                &mut *tx,
                trade,
                evidence.winner_hash,
                evidence.transaction_time,
                evidence.transaction_time,
                evidence.transaction_fee,
                evidence.resource_consume,
            )
            .await?;
            let updated = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, trade).await?;
            ApiCollectDao::update_status(&mut *tx, trade, updated.recompute_status()).await?;
        }
        sqlx::query("UPDATE api_collect_replacement SET winner_hash=?,execution_success=?,actual_transaction_fee=?,actual_resource_consume=?,block_hash=?,block_number=?,transaction_time=?,updated_at=CURRENT_TIMESTAMP WHERE new_trade_no=? AND winner_hash IS NULL")
            .bind(evidence.winner_hash).bind(evidence.success).bind(evidence.transaction_fee).bind(evidence.resource_consume).bind(evidence.block_hash).bind(evidence.block_number).bind(evidence.transaction_time).bind(trade).execute(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        // Normalize only the captured legacy high-water, in the same writer
        // transaction as winner/execution facts. api_nonce stores LAST USED,
        // so a confirmed nonce N writes N, leaving N+1 free until a signed commit owns it.
        // Retain the original high-water audit in legacy_counter.
        // All three signed-intent flows are checked; late unknown changes or
        // higher ownership must never be erased by a repeated recovery.
        let next = r
            .nonce
            .checked_add(1)
            .ok_or_else(|| crate::Error::Other("replacement nonce overflow".into()))?;
        let counter =
            ApiNonceDao::get_api_nonce_optional(&mut *tx, &r.from_addr, &r.chain_code).await?;
        if counter == Some(r.legacy_counter) {
            let claims =
                ApiNonceDao::nonce_claims(&mut *tx, &r.from_addr, &r.chain_code, next, i64::MAX)
                    .await?;
            // This is a one-time correction of the exact captured legacy floor.
            // Preserve every later signed/uncertain owner, even when its nonce is higher.
            let last_owned = claims.iter().map(|c| c.nonce).max().unwrap_or(r.nonce).max(r.nonce);
            sqlx::query("UPDATE api_nonce SET nonce=?,updated_at=CURRENT_TIMESTAMP WHERE lower(trim(from_addr))=lower(trim(?)) AND chain_code=? AND nonce<=?")
                .bind(last_owned).bind(&r.from_addr).bind(&r.chain_code).bind(r.legacy_counter)
                .execute(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?;
        }
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "collect_replacement_tests.rs"]
mod tests;
