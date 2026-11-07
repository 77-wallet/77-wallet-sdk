use super::ReturnType;
use crate::{
    manager::WalletManager,
    request::transaction::{self},
    response_vo::{
        self, CoinCurrency,
        standard_wallet::{
            account::Balance,
            transaction::{BillDetailVo, TransactionResult},
        },
    },
    service::{bill::BillService, transaction::TransactionService},
};
use wallet_database::{
    entities::{
        asset_token_key::AssetTokenKey,
        bill::{BillEntity, BillKind, RecentBillListVo},
    },
    pagination::Pagination,
};

impl WalletManager {
    // 本币的余额
    pub async fn chain_balance(
        &self,
        address: &str,
        chain_code: &str,
        symbol: &str,
        token_address: Option<String>,
    ) -> ReturnType<Balance> {
        TransactionService::new(self.ctx)
            .chain_balance(
                address,
                chain_code,
                symbol,
                AssetTokenKey::from_raw(token_address.as_deref()),
            )
            .await
    }

    /// Estimates the transaction fee for a transfer request.
    pub async fn transaction_fee(
        &self,
        req: transaction::BaseTransferReq,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        TransactionService::new(self.ctx).transaction_fee(req).await
    }

    /// tokenAddress前端必须传
    pub async fn transfer(&self, req: transaction::TransferReq) -> ReturnType<TransactionResult> {
        TransactionService::new(self.ctx).transfer(req, BillKind::Transfer).await
    }

    pub async fn bill_detail(&self, tx_hash: &str, owner: &str) -> ReturnType<BillDetailVo> {
        TransactionService::new(self.ctx).bill_detail(tx_hash, owner).await
    }

    pub async fn list_by_hashs(
        &self,
        owner: String,
        hashs: Vec<String>,
    ) -> ReturnType<Vec<BillEntity>> {
        TransactionService::new(self.ctx).list_by_hashs(owner, hashs).await
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
        transfer_type: Vec<i32>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<BillEntity>> {
        let service = BillService::new(self.ctx)?;
        service
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
                transfer_type,
                page,
                page_size,
            )
            .await
    }

    // 最近交易列表
    pub async fn recent_bill(
        &self,
        token: String,
        addr: String,
        chain_code: String,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<RecentBillListVo>> {
        TransactionService::new(self.ctx)
            .recent_bill(&token, &addr, &chain_code, page, page_size)
            .await
    }

    // 单笔查询交易并处理
    pub async fn query_tx_result(&self, req: Vec<String>) -> ReturnType<Vec<BillEntity>> {
        TransactionService::new(self.ctx).query_tx_result(req).await
    }

    pub async fn sync_bill(&self, chain_code: String, address: String) -> ReturnType<()> {
        let service = BillService::new(self.ctx)?;
        service.sync_bill_by_address(&chain_code, &address).await
    }

    pub async fn sync_bill_by_wallet_and_account(
        &self,
        wallet_address: String,
        account_id: u32,
    ) -> ReturnType<()> {
        let service = BillService::new(self.ctx)?;
        service.sync_bill_by_wallet_and_account(wallet_address, account_id).await
    }

    // 币汇率
    pub async fn coin_currency_price(
        &self,
        chain_code: String,
        symbol: String,
        token_address: Option<String>,
    ) -> ReturnType<CoinCurrency> {
        let service = BillService::new(self.ctx)?;
        service
            .coin_currency_price(
                chain_code,
                symbol,
                AssetTokenKey::from_raw(token_address.as_deref()),
            )
            .await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::{request::transaction::BaseTransferReq, testkit::env::get_manager};
    use anyhow::Result;

    #[tokio::test]
    async fn test_trasaction_fee() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let from = "0x4f31D44C05d6fDce4db64da2E9601BeE8ad9EA5e";
        let to = "0x4f31D44C05d6fDce4db64da2E9601BeE8ad9EA5e";
        let value = "0.000001";
        let chain_code = "eth";
        let symbol = "USDT";
        // let symbol = "USDT";

        let mut params = BaseTransferReq::new(from, to, value, chain_code, symbol);
        params.with_token(Some("0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string()));

        let res = wallet_manager.transaction_fee(params).await?;
        tracing::info!("token_fee: {}", serde_json::to_string(&res).unwrap());

        Ok(())
    }

    #[tokio::test]
    async fn test_chain_balance() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let address = "TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF";
        let chain_code = "tron";
        // let symbol = "USDT";
        let symbol = "TRX";
        // let token_address = Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string());
        let token_address = Some("".to_string());
        // let symbol = "USDT";

        let res = wallet_manager.chain_balance(address, chain_code, symbol, token_address).await?;
        tracing::info!("token_fee: {}", serde_json::to_string(&res).unwrap());

        Ok(())
    }

    #[tokio::test]
    async fn test_transfer() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;
        let from = "TNcqVubRFAJDMAzUPh4uDBtTmgkj76mMgF";
        let to = "TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF";
        let value = "1";
        let symbol = "TRX";
        let chain_code = "tron";
        let password = "q1111111";
        // let password = "1111111";
        let notes = "test".to_string();

        let mut base = BaseTransferReq::new(from, to, value, chain_code, symbol);
        base.with_notes(notes);
        let params = crate::request::transaction::TransferReq {
            base,
            password: password.to_string(),
            fee_setting: "".to_string(),
            signer: None,
        };

        let res = wallet_manager.transfer(params).await?;
        tracing::info!("transfer: {}", serde_json::to_string(&res).unwrap());

        Ok(())
    }
}
