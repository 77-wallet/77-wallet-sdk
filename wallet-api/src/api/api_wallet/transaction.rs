use crate::{
    api::ReturnType,
    manager::WalletManager,
    request::{
        api_wallet::transfer::ApiTransferExReq,
        transaction::{self},
    },
    response_vo::{
        api_wallet::transaction::ApiBillDetailVo, standard_wallet::transaction::TransactionResult,
    },
    service::{api_wallet::transaction::ApiTransService, transaction::TransactionService},
};
use serde_json;
use wallet_database::{
    entities::bill::{BillEntity, BillKind, RecentBillListVo},
    pagination::Pagination,
};

impl WalletManager {
    /// Estimates the transaction fee for a transfer request.
    pub async fn api_trans_fee(
        &self,
        req: transaction::BaseTransferReq,
    ) -> ReturnType<crate::response_vo::EstimateFeeResp> {
        TransactionService::new(self.ctx).transaction_fee(req).await
    }

    /// tokenAddress前端必须传
    pub async fn api_transfer(&self, req: ApiTransferExReq) -> ReturnType<TransactionResult> {
        ApiTransService::new(self.ctx).transfer(req, BillKind::Transfer).await
    }

    #[cfg(any(test, feature = "integration-tests"))]
    pub async fn api_transfer_with_preloaded_private_key(
        &self,
        req: ApiTransferExReq,
        private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> ReturnType<TransactionResult> {
        crate::domain::wallet::WalletDomain::validate_password_with_context(
            self.ctx,
            &req.password,
        )
        .await?;
        ApiTransService::new(self.ctx).transfer_with_private_key(req, private_key).await
    }

    pub async fn api_bill_detail(&self, tx_hash: &str, owner: &str) -> ReturnType<ApiBillDetailVo> {
        ApiTransService::new(self.ctx).bill_detail(tx_hash, owner).await
    }

    pub async fn api_list_by_hashs(
        &self,
        owner: String,
        hashs: Vec<String>,
    ) -> ReturnType<Vec<BillEntity>> {
        ApiTransService::new(self.ctx).list_by_hashs(hashs, &owner).await
    }

    pub async fn api_bill_lists(
        &self,
        root_addr: Option<String>,
        account_id: Option<u32>,
        is_multisig: Option<i64>,
        addr: Option<String>,
        chain_code: Option<String>,
        symbol: Option<String>,
        filter_min_value: Option<bool>,
        start: Option<i64>,
        end: Option<i64>,
        tx_kind: Vec<i32>,
        transfer_type: Option<i32>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<BillEntity>> {
        ApiTransService::new(self.ctx)
            .bill_lists(
                root_addr,
                account_id,
                addr,
                chain_code.as_deref(),
                symbol.as_deref(),
                is_multisig,
                filter_min_value,
                start,
                end,
                tx_kind,
                transfer_type,
                page,
                page_size,
            )
            .await
    }

    // 最近交易列表
    pub async fn api_recent_bill(
        &self,
        token: &str,
        addr: &str,
        chain_code: &str,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<RecentBillListVo>> {
        ApiTransService::new(self.ctx).recent_bill(token, addr, chain_code, page, page_size).await
    }

    // // 单笔查询交易并处理
    pub async fn api_query_tx_result(&self, req: Vec<String>) -> ReturnType<Vec<BillEntity>> {
        ApiTransService::new(self.ctx).query_tx_result(req).await
    }

    /// 统计归集订单执行耗时
    pub async fn api_collect_order_stats(&self) -> ReturnType<serde_json::Value> {
        use sqlx::Row;

        // 获取数据库连接池
        let pool = self.ctx.api_transaction_pool()?;

        // 1. 每笔订单的执行耗时
        let per_order_stats = sqlx::query(
            r#"
            SELECT 
                trade_no, 
                order_ack_sent_at    AS start_at, 
                finished_at          AS end_at, 
                CAST( 
                    (julianday(finished_at) - julianday(order_ack_sent_at)) * 86400 
                    AS INTEGER 
                ) AS cost_seconds 
            FROM api_collect 
            WHERE 
                order_ack_sent_at IS NOT NULL 
                AND finished_at IS NOT NULL 
            ORDER BY cost_seconds DESC
        "#,
        )
        .fetch_all(pool.as_ref())
        .await
        .unwrap();

        let mut per_order_stats_list = Vec::new();
        for row in per_order_stats {
            let trade_no: String = row.try_get("trade_no").unwrap();
            let start_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("start_at").unwrap();
            let end_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("end_at").unwrap();
            let cost_seconds: i64 = row.try_get("cost_seconds").unwrap();

            per_order_stats_list.push(serde_json::json!({
                "trade_no": trade_no,
                "start_at": start_at,
                "end_at": end_at,
                "cost_seconds": cost_seconds
            }));
        }

        // 2. 每一阶段耗时
        let stage_stats = sqlx::query(
            r#"
            SELECT 
                trade_no, 
                CAST((julianday(building_at) - julianday(order_ack_sent_at)) * 86400 AS INTEGER) 
                    AS build_cost_sec, 
                CAST((julianday(last_broadcast_at) - julianday(building_at)) * 86400 AS INTEGER) 
                    AS broadcast_cost_sec, 
                CAST((julianday(finished_at) - julianday(last_broadcast_at)) * 86400 AS INTEGER) 
                    AS chain_confirm_cost_sec 
            FROM api_collect 
            WHERE 
                order_ack_sent_at IS NOT NULL 
                AND building_at IS NOT NULL 
                AND last_broadcast_at IS NOT NULL 
                AND finished_at IS NOT NULL 
            ORDER BY chain_confirm_cost_sec DESC
        "#,
        )
        .fetch_all(pool.as_ref())
        .await
        .unwrap();

        let mut stage_stats_list = Vec::new();
        for row in stage_stats {
            let trade_no: String = row.try_get("trade_no").unwrap();
            let build_cost_sec: i64 = row.try_get("build_cost_sec").unwrap();
            let broadcast_cost_sec: i64 = row.try_get("broadcast_cost_sec").unwrap();
            let chain_confirm_cost_sec: i64 = row.try_get("chain_confirm_cost_sec").unwrap();

            stage_stats_list.push(serde_json::json!({
                "trade_no": trade_no,
                "build_cost_sec": build_cost_sec,
                "broadcast_cost_sec": broadcast_cost_sec,
                "chain_confirm_cost_sec": chain_confirm_cost_sec
            }));
        }

        // 3. 整批订单的总耗时
        let total_stats = sqlx::query(
            r#"
            SELECT 
                MIN(order_ack_sent_at) AS first_order_start, 
                MAX(finished_at)       AS last_order_end, 
                CAST( 
                    (julianday(MAX(finished_at)) - julianday(MIN(order_ack_sent_at))) * 86400 
                    AS INTEGER 
                ) AS total_cost_seconds 
            FROM api_collect 
            WHERE 
                order_ack_sent_at IS NOT NULL 
                AND finished_at IS NOT NULL
        "#,
        )
        .fetch_optional(pool.as_ref())
        .await
        .unwrap();

        let total_stats_json = if let Some(row) = total_stats {
            let first_order_start: Option<chrono::DateTime<chrono::Utc>> =
                row.try_get("first_order_start").unwrap();
            let last_order_end: Option<chrono::DateTime<chrono::Utc>> =
                row.try_get("last_order_end").unwrap();
            let total_cost_seconds: i64 = row.try_get("total_cost_seconds").unwrap();

            serde_json::json!({
                "first_order_start": first_order_start,
                "last_order_end": last_order_end,
                "total_cost_seconds": total_cost_seconds
            })
        } else {
            serde_json::json!({
                "first_order_start": null,
                "last_order_end": null,
                "total_cost_seconds": 0
            })
        };

        // 4. 组装结果
        let result = serde_json::json!({
            "per_order_stats": per_order_stats_list,
            "stage_stats": stage_stats_list,
            "total_stats": total_stats_json
        });

        Ok(result)
    }
}
