use super::ReturnType;
use crate::request::transaction::{self, QueryBillReusltReq};
use crate::response_vo::CoinCurrency;
use crate::response_vo::{
    self,
    account::Balance,
    transaction::{BillDetailVo, TransactionResult},
};
use crate::service::bill::BillService;
use wallet_database::entities::bill::{BillEntity, BillKind, RecentBillListVo};
use wallet_database::pagination::Pagination;

impl crate::WalletManager {
    // 本币的余额
    pub async fn chain_balance(
        &self,
        address: &str,
        chain_code: &str,
        symbol: &str,
    ) -> ReturnType<Balance> {
        crate::service::transaction::TransactionService::chain_balance(address, chain_code, symbol)
            .await
            .into()
    }

    /// Estimates the transaction fee for a transfer request.
    pub async fn transaction_fee(
        &self,
        req: transaction::BaseTransferReq,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        crate::service::transaction::TransactionService::transaction_fee(req)
            .await
            .into()
    }

    pub async fn transfer(&self, req: transaction::TransferReq) -> ReturnType<TransactionResult> {
        crate::service::transaction::TransactionService::transfer(req, BillKind::Transfer)
            .await
            .into()
    }

    pub async fn bill_detail(&self, tx_hash: &str, transfer_type: i64) -> ReturnType<BillDetailVo> {
        crate::service::transaction::TransactionService::bill_detail(tx_hash, transfer_type)
            .await
            .into()
    }

    pub async fn bill_lists(
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
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<BillEntity>> {
        BillService::new(self.repo_factory.resuource_repo())
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
                page,
                page_size,
            )
            .await?
            .into()
    }

    // 最近交易列表
    pub async fn recent_bill(
        &self,
        symbol: String,
        addr: String,
        chain_code: String,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<RecentBillListVo>> {
        crate::service::transaction::TransactionService::recent_bill(
            &symbol,
            &addr,
            &chain_code,
            page,
            page_size,
        )
        .await
        .into()
    }

    // 单笔查询交易并处理
    pub async fn query_tx_result(
        &self,
        req: Vec<QueryBillReusltReq>,
    ) -> ReturnType<Vec<BillEntity>> {
        crate::service::transaction::TransactionService::query_tx_result(req)
            .await
            .into()
    }

    pub async fn sync_bill(&self, chain_code: String, address: String) -> ReturnType<()> {
        BillService::new(self.repo_factory.resuource_repo())
            .sync_bill_by_address(&chain_code, &address)
            .await?
            .into()
    }

    pub async fn sync_bill_by_wallet_and_account(
        &self,
        wallet_address: String,
        account_id: u32,
    ) -> ReturnType<()> {
        BillService::new(self.repo_factory.resuource_repo())
            .sync_bill_by_wallet_and_account(wallet_address, account_id)
            .await?
            .into()
    }

    // 币汇率
    pub async fn coin_currency_price(
        &self,
        chain_code: String,
        symbol: String,
    ) -> ReturnType<CoinCurrency> {
        BillService::new(self.repo_factory.resuource_repo())
            .coin_currency_price(chain_code, symbol)
            .await
            .into()
    }
}
