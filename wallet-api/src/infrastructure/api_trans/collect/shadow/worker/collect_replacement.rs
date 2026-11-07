//! Automatic recovery reuses the existing durable collect replacement lifecycle.
use super::ShadowCollectWorker;
use crate::{
    domain::api_wallet::{
        adapter::eth_tx::replacement::ReplacementFeeContext,
        adapter_factory::ApiChainAdapterFactory,
    },
    error::service::ServiceError,
};
use wallet_database::{
    entities::api_collect::ApiCollectEntity,
    repositories::api_wallet::collect_replacement::{
        CollectReplacementRepo, CollectReplacementScope, ReplacementResolution,
    },
};

pub(super) enum ReplacementProgress {
    Normal,
    Wait,
    ContinueNew,
    Stop,
}
impl ShadowCollectWorker {
    async fn active_replacement_scope(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<Option<CollectReplacementScope>, ServiceError> {
        let chain: wallet_types::chain::chain::ChainCode = req.chain_code.as_str().try_into()?;
        if !crate::domain::chain::is_evm_chain(chain) || req.token_addr.is_contract() {
            return Ok(None);
        }
        let scope =
            CollectReplacementRepo::discover(&self.ctx.api_transaction_pool()?, &req.trade_no)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
        let Some(scope) = scope else {
            return Ok(None);
        };
        if CollectReplacementRepo::scope_is_resolved(&self.ctx.api_transaction_pool()?, &scope)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?
        {
            return Ok(None);
        }
        Ok(Some(scope))
    }
    pub(super) async fn scoped_collect_nonce_before_fee(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<Option<(u64, i64, Option<ReplacementFeeContext>)>, ServiceError> {
        let discovered = self.active_replacement_scope(req).await?.is_some();
        let existing =
            CollectReplacementRepo::find(&self.ctx.api_transaction_pool()?, &req.trade_no)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?
                .is_some();
        if !discovered && !existing {
            return Ok(None);
        }
        // A malformed order must not claim the old signed slot before the fee gate.
        if !self.check_digest(req).await? {
            return Err(ServiceError::Parameter(
                "scoped replacement digest verification failed".into(),
            ));
        }
        self.observe_collect_nonce(req).await.map(Some)
    }
    pub(super) async fn observe_collect_nonce(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<(u64, i64, Option<ReplacementFeeContext>), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let existing = CollectReplacementRepo::find(&pool, &req.trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
        let scope = self.active_replacement_scope(req).await?;
        if existing.is_none() && scope.is_none() {
            return self
                .get_nonce(&req.from_addr, &req.chain_code)
                .await
                .map(|(n, c)| (n, c, None));
        }
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, &req.chain_code)
                .await?;
        let chain_next = adapter.replacement_latest_nonce(&req.from_addr).await?;
        let chain_next = i64::try_from(chain_next)
            .map_err(|_| ServiceError::Parameter("nonce exceeds recovery range".into()))?;
        let r = if let Some(r) = existing {
            r
        } else {
            let scope = scope.ok_or_else(|| {
                ServiceError::Parameter("missing discovered replacement identity".into())
            })?;
            // A future signature must not pull a new order past a still-missing lower nonce.
            if chain_next < scope.nonce {
                return self
                    .get_nonce(&req.from_addr, &req.chain_code)
                    .await
                    .map(|(n, c)| (n, c, None));
            }
            let Some(r) = CollectReplacementRepo::prepare(&pool, &req.trade_no, &scope, chain_next)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?
            else {
                return Err(ServiceError::Parameter(
                    "scoped replacement is not eligible or owned by another order".into(),
                ));
            };
            r
        };
        match self.reconcile_collect_replacement(req).await? {
            ReplacementProgress::Stop | ReplacementProgress::ContinueNew => {
                return Err(ServiceError::Parameter(
                    "replacement nonce already consumed; result reconciled".into(),
                ));
            }
            _ => {}
        }
        if chain_next != r.nonce {
            return Err(ServiceError::Parameter(
                "replacement nonce consumed but canonical winner not yet visible".into(),
            ));
        }
        Ok((
            r.nonce as u64,
            chain_next,
            Some(ReplacementFeeContext {
                old_raw: r.old_raw,
                old_hash: r.old_hash,
                from_addr: r.from_addr,
                nonce: r.nonce as u64,
            }),
        ))
    }
    pub(super) async fn reconcile_collect_replacement(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<ReplacementProgress, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let Some(r) = CollectReplacementRepo::find(&pool, &req.trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?
        else {
            return Ok(ReplacementProgress::Normal);
        };
        if r.abandoned_at.is_some() {
            return Ok(ReplacementProgress::Stop);
        }
        if let Some(winner) = r.winner_hash {
            if r.new_hash.as_deref() == Some(winner.as_str()) && r.execution_success == Some(true) {
                return Ok(ReplacementProgress::ContinueNew);
            }
            self.advancer.try_advance(&req.trade_no).await;
            return Ok(ReplacementProgress::Stop);
        }
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, &r.chain_code)
                .await?;
        let old = adapter.replacement_receipt(&r.old_hash, &r.from_addr, r.nonce as u64).await;
        let new = if let Some(hash) = r.new_hash.as_deref() {
            adapter.replacement_receipt(hash, &r.from_addr, r.nonce as u64).await
        } else {
            Ok(None)
        };
        let old = old.unwrap_or_else(|error| {
            tracing::warn!(trade_no=%req.trade_no,tx_hash=%r.old_hash,error=%error,"Old replacement identity proof unavailable; retaining ownership");
            None
        });
        let new = new.unwrap_or_else(|error| {
            tracing::warn!(trade_no=%req.trade_no,tx_hash=?r.new_hash,error=%error,"New replacement identity proof unavailable; retaining ownership");
            None
        });
        if old.is_some() && new.is_some() {
            return Err(ServiceError::Parameter(
                "inconsistent replacement chain evidence: both identities included".into(),
            ));
        }
        let mut winner = old.or(new);
        if winner.is_none()
            && adapter
                .replacement_latest_nonce(&r.from_addr)
                .await
                .is_ok_and(|n| n > r.nonce as u64)
        {
            if let Ok(Some(conflict)) = adapter
                .finalized_nonce_conflict(&r.chain_code, &r.from_addr, r.nonce as i32, &r.old_hash)
                .await
            {
                winner = adapter
                    .replacement_receipt(
                        &conflict.conflicting_tx_hash,
                        &r.from_addr,
                        r.nonce as u64,
                    )
                    .await
                    .ok()
                    .flatten();
            }
        }
        let Some(w) = winner else { return Ok(ReplacementProgress::Wait) };
        let block = i64::try_from(w.block_number)
            .map_err(|_| ServiceError::Parameter("replacement proof height overflow".into()))?;
        CollectReplacementRepo::resolve(
            &pool,
            &req.trade_no,
            &ReplacementResolution {
                transaction_fee: &w.transaction_fee,
                resource_consume: &w.resource_consume,
                winner_hash: &w.tx_hash,
                success: w.success,
                block_hash: &w.block_hash,
                block_number: block,
                transaction_time: &w.transaction_time,
            },
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        self.advancer.try_advance(&req.trade_no).await;
        if r.new_hash.as_deref() == Some(w.tx_hash.as_str()) && w.success {
            Ok(ReplacementProgress::ContinueNew)
        } else {
            self.advancer.try_advance(&req.trade_no).await;
            Ok(ReplacementProgress::Stop)
        }
    }
}
