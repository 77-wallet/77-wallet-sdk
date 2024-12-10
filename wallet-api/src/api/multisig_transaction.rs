use crate::{
    api::ReturnType,
    response_vo::{
        self, account::Balance, multisig_account::QueueInfo,
        multisig_transaction::MultisigQueueInfoVo, transaction::TransferParams,
        MultisigQueueFeeParams,
    },
};
use wallet_database::pagination::Pagination;

impl crate::WalletManager {
    pub async fn create_queue_fee(
        &self,
        params: MultisigQueueFeeParams,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        crate::service::multisig_transaction::MultisigTransactionService::create_queue_fee(params)
            .await
            .into()
    }

    /// Creates a new multisig transaction with the provided parameters.
    pub async fn create_multisig_queue(&self, params: TransferParams) -> ReturnType<String> {
        crate::service::multisig_transaction::MultisigTransactionService::create_multisig_queue(
            params,
        )
        .await
        .into()
    }

    pub async fn multisig_queue_list(
        &self,
        from: Option<String>,
        chain_code: Option<String>,
        status: i32,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<MultisigQueueInfoVo>> {
        crate::service::multisig_transaction::MultisigTransactionService::multisig_queue_list(
            from.as_deref(),
            chain_code.as_deref(),
            status,
            page,
            page_size,
        )
        .await
        .into()
    }

    pub async fn multisig_queue_info(&self, queue_id: String) -> ReturnType<MultisigQueueInfoVo> {
        crate::service::multisig_transaction::MultisigTransactionService::multisig_queue_info(
            &queue_id,
        )
        .await
        .into()
    }

    pub async fn sign_fee(
        &self,
        queue_id: String,
        address: String,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        crate::service::multisig_transaction::MultisigTransactionService::sign_fee(
            queue_id, address,
        )
        .await
        .into()
    }

    pub async fn sign_transaction(
        &self,
        queue_id: String,
        status: i32,
        password: String,
        address: Option<String>,
    ) -> ReturnType<()> {
        crate::service::multisig_transaction::MultisigTransactionService::sign_multisig_transaction(
            &queue_id, status, &password, address,
        )
        .await
        .into()
    }

    pub async fn estimate_multisig_transfer_fee(
        &self,
        queue_id: String,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        crate::service::multisig_transaction::MultisigTransactionService::multisig_transfer_fee(
            &queue_id,
        )
        .await
        .into()
    }

    pub async fn exec_transaction(
        &self,
        queue_id: String,
        password: String,
        fee_setting: Option<String>,
        request_resource_id: Option<String>,
    ) -> ReturnType<String> {
        crate::service::multisig_transaction::MultisigTransactionService::exec_multisig_transaction(
            &queue_id,
            password,
            fee_setting,
            request_resource_id,
        )
        .await
        .into()
    }

    // 多签的余额
    pub async fn multi_account_balance(
        &self,
        address: String,
        chain_code: String,
        symbol: String,
    ) -> ReturnType<Balance> {
        crate::service::transaction::TransactionService::chain_balance(
            &address,
            &chain_code,
            &symbol,
        )
        .await
        .into()
    }

    pub async fn check_ongoing_queue(
        &self,
        chain_code: String,
        address: String,
    ) -> ReturnType<Option<QueueInfo>> {
        crate::service::multisig_transaction::MultisigTransactionService::check_ongoing_queue(
            chain_code, address,
        )
        .await
        .into()
    }

    pub async fn cancel_queue(&self, queue_id: String) -> ReturnType<()> {
        crate::service::multisig_transaction::MultisigTransactionService::cancel_queue(queue_id)
            .await
            .into()
    }
}
