use crate::{
    api::ReturnType,
    manager::WalletManager,
    response_vo::{
        self, MultisigQueueFeeParams,
        standard_wallet::{
            account::Balance, multisig_account::QueueInfo,
            multisig_transaction::MultisigQueueInfoVo, transaction::TransferParams,
        },
    },
    service::multisig_transaction::MultisigTransactionService,
};
use wallet_database::{entities::asset_token_key::AssetTokenKey, pagination::Pagination};

impl WalletManager {
    // only solana has create fee
    pub async fn create_queue_fee(
        &self,
        params: MultisigQueueFeeParams,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        MultisigTransactionService::new(self.ctx).create_queue_fee(params).await
    }

    /// Creates a new multisig transaction with the provided parameters.
    pub async fn create_multisig_queue(
        &self,
        params: TransferParams,
        password: String,
    ) -> ReturnType<String> {
        MultisigTransactionService::new(self.ctx).create_multisig_queue(params, password).await
    }

    pub async fn multisig_queue_list(
        &self,
        from: Option<String>,
        chain_code: Option<String>,
        status: i32,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<MultisigQueueInfoVo>> {
        MultisigTransactionService::new(self.ctx)
            .multisig_queue_list(from.as_deref(), chain_code.as_deref(), status, page, page_size)
            .await
    }

    pub async fn multisig_queue_info(&self, queue_id: String) -> ReturnType<MultisigQueueInfoVo> {
        MultisigTransactionService::new(self.ctx).multisig_queue_info(&queue_id).await.into()
    }

    pub async fn sign_fee(
        &self,
        queue_id: String,
        address: String,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        MultisigTransactionService::new(self.ctx).sign_fee(queue_id, address).await.into()
    }

    pub async fn sign_transaction(
        &self,
        queue_id: String,
        status: i32,
        password: String,
        address: Option<String>,
    ) -> ReturnType<()> {
        MultisigTransactionService::new(self.ctx)
            .sign_multisig_transaction(&queue_id, status, &password, address)
            .await
    }

    pub async fn estimate_multisig_transfer_fee(
        &self,
        queue_id: String,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        MultisigTransactionService::new(self.ctx).multisig_transfer_fee(&queue_id).await
    }

    pub async fn exec_transaction(
        &self,
        queue_id: String,
        password: String,
        fee_setting: Option<String>,
        request_resource_id: Option<String>,
    ) -> ReturnType<String> {
        MultisigTransactionService::new(self.ctx)
            .exec_multisig_transaction(&queue_id, password, fee_setting, request_resource_id)
            .await
    }

    // 多签的余额
    pub async fn multi_account_balance(
        &self,
        address: String,
        chain_code: String,
        symbol: String,
        token_address: Option<String>,
    ) -> ReturnType<Balance> {
        crate::service::transaction::TransactionService::new(self.ctx)
            .chain_balance(
                &address,
                &chain_code,
                &symbol,
                AssetTokenKey::from_raw(token_address.as_deref()),
            )
            .await
    }

    pub async fn check_ongoing_queue(
        &self,
        chain_code: String,
        address: String,
    ) -> ReturnType<Option<QueueInfo>> {
        MultisigTransactionService::new(self.ctx).check_ongoing_queue(chain_code, address).await
    }

    pub async fn cancel_queue(&self, queue_id: String) -> ReturnType<()> {
        MultisigTransactionService::new(self.ctx).cancel_queue(queue_id).await
    }
}
