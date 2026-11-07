use crate::{
    context::Context, error::service::ServiceError,
    infrastructure::api_trans::collect_fee::shadow::ShadowFeeWorker,
    request::api_wallet::trans::ApiBaseTransferReq,
};

/// Test-facing wrapper for the Solana fee rent helper.
///
/// Keeping this in `testkit` allows integration tests to exercise the
/// behavior without embedding test code inside the deprecated fee worker module.
pub async fn bump_sol_native_transfer_value_for_rent(
    ctx: &'static Context,
    params: &mut ApiBaseTransferReq,
    symbol: &str,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let ctx: &'static Context = ctx;
    ShadowFeeWorker::bump_sol_native_transfer_value_for_rent(ctx, params, symbol, trade_no).await
}

/// Test-facing wrapper for the Solana rent error detector.
pub fn is_solana_recipient_rent_error(err: &ServiceError) -> bool {
    ShadowFeeWorker::is_solana_recipient_rent_error(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::api_wallet::{RawTx, Tx},
        error::business::{
            BusinessError,
            chain::{ChainError, InsufficientBalanceDetail},
        },
        request::api_wallet::trans::ApiBaseTransferReq,
        testkit::adapter_factory::{
            clear_test_transaction_adapter_override, set_test_transaction_adapter_override,
        },
    };
    use alloy::primitives::U256;
    use serial_test::serial;
    use std::sync::Arc;
    use wallet_chain_interact::{Error, QueryTransactionResult};
    use wallet_database::entities::asset_token_key::AssetTokenKey;

    #[derive(Clone)]
    struct RentAwareAdapter {
        return_rent_error: bool,
    }

    #[async_trait::async_trait]
    impl Tx for RentAwareAdapter {
        async fn account_resource(
            &self,
            _owner_address: &str,
        ) -> Result<
            wallet_chain_interact::tron::protocol::account::AccountResourceDetail,
            ServiceError,
        > {
            unimplemented!()
        }

        async fn balance_token_key(
            &self,
            _addr: &str,
            _token: AssetTokenKey,
        ) -> Result<U256, Error> {
            Ok(U256::ZERO)
        }

        async fn nonce(&self, _addr: &str) -> Result<u64, ServiceError> {
            Ok(0)
        }

        async fn block_num(&self) -> Result<u64, Error> {
            Ok(0)
        }

        async fn query_tx_res(&self, _hash: &str) -> Result<Option<QueryTransactionResult>, Error> {
            Ok(None)
        }

        async fn token_symbol(&self, _token: &str) -> Result<String, Error> {
            Ok("SOL".to_string())
        }

        async fn token_name(&self, _token: &str) -> Result<String, Error> {
            Ok("Solana".to_string())
        }

        async fn decimals(&self, _token: &str) -> Result<u8, Error> {
            Ok(9)
        }

        async fn black_address(&self, _token: &str, _owner: &str) -> Result<bool, ServiceError> {
            Ok(false)
        }

        async fn transfer(
            &self,
            _params: &crate::request::api_wallet::trans::ApiTransferReq,
            _private_key: wallet_chain_interact::types::ChainPrivateKey,
        ) -> Result<crate::domain::chain::TransferResp, ServiceError> {
            unimplemented!()
        }

        async fn estimate_fee(
            &self,
            req: ApiBaseTransferReq,
            _main_symbol: &str,
        ) -> Result<String, ServiceError> {
            if self.return_rent_error && req.token_address.is_native() {
                return Err(ServiceError::Business(BusinessError::Chain(
                    ChainError::insufficient_balance_with_detail(
                        InsufficientBalanceDetail::new().reason(
                            "recipient account is not initialized and transfer amount is below rent-exempt minimum",
                        ),
                    ),
                )));
            }

            Ok(r#"{"estimateFee":{"amount":"0.000015","currency":"USD","unitPrice":0.0,"fiatValue":0.0}}"#
                .to_string())
        }

        async fn build_transfer_raw(
            &self,
            _params: &crate::request::api_wallet::trans::ApiTransferReq,
            _private_key: wallet_chain_interact::types::ChainPrivateKey,
        ) -> Result<(String, RawTx, String), ServiceError> {
            unimplemented!()
        }

        async fn broadcast_transfer(
            &self,
            _raw: RawTx,
        ) -> Result<crate::domain::chain::TransferResp, ServiceError> {
            unimplemented!()
        }
    }

    struct AdapterGuard;

    impl Drop for AdapterGuard {
        fn drop(&mut self) {
            clear_test_transaction_adapter_override("sol");
        }
    }

    fn install_adapter(return_rent_error: bool) -> AdapterGuard {
        let adapter: Arc<dyn Tx + Send + Sync> = Arc::new(RentAwareAdapter { return_rent_error });
        set_test_transaction_adapter_override("sol", adapter);
        AdapterGuard
    }

    #[test]
    fn solana_recipient_rent_error_detector_matches_precheck_message() {
        let err = ServiceError::Business(BusinessError::Chain(
            ChainError::insufficient_balance_with_detail(InsufficientBalanceDetail::new().reason(
                "recipient account is not initialized and transfer amount is below rent-exempt minimum",
            )),
        ));

        assert!(is_solana_recipient_rent_error(&err));
    }

    #[test]
    fn solana_recipient_rent_error_detector_ignores_other_errors() {
        let err = ServiceError::Parameter("other error".into());
        assert!(!is_solana_recipient_rent_error(&err));
    }

    #[serial]
    #[tokio::test]
    async fn bump_sol_native_transfer_value_for_rent_increases_value_when_rent_error_is_detected() {
        let _guard = install_adapter(true);
        let mut params = ApiBaseTransferReq::new(
            "from",
            "to",
            "0.000015",
            &wallet_types::chain::chain::ChainCode::Solana.to_string(),
        );
        params.with_token(AssetTokenKey::Native, 9, "SOL");

        let ctx = crate::testkit::context::api_trans_test_ctx().await;

        bump_sol_native_transfer_value_for_rent(ctx, &mut params, "SOL", "T1")
            .await
            .expect("bump rent");

        let bumped = wallet_utils::conversion::decimal_from_str(&params.value).unwrap();
        let original = wallet_utils::conversion::decimal_from_str("0.000015").unwrap();
        assert!(bumped > original);
    }

    #[serial]
    #[tokio::test]
    async fn bump_sol_native_transfer_value_for_rent_keeps_value_when_estimate_succeeds() {
        let _guard = install_adapter(false);
        let mut params = ApiBaseTransferReq::new(
            "from",
            "to",
            "1.0",
            &wallet_types::chain::chain::ChainCode::Solana.to_string(),
        );
        params.with_token(AssetTokenKey::Contract("mint".to_string()), 6, "USDC");

        let ctx = crate::testkit::context::api_trans_test_ctx().await;

        bump_sol_native_transfer_value_for_rent(ctx, &mut params, "USDC", "T2")
            .await
            .expect("no bump needed");

        assert_eq!(params.value, "1.0");
    }
}

/// Exercise collect-fee BuildTx's read-only candidate observation without signing.
pub async fn allocate_collect_fee_nonce(
    ctx: &'static Context,
    address: &str,
    chain: &str,
) -> Result<u64, ServiceError> {
    use crate::infrastructure::api_trans::{
        AddressLockManager,
        collect_fee::shadow::{ScannerConfig, ShadowScanner},
    };
    use std::sync::Arc;
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None));
    ShadowFeeWorker::new(ctx, Arc::new(AddressLockManager::new()), scanner)
        .get_nonce(address, chain)
        .await
        .map(|(nonce, _)| nonce)
}

/// Testkit owns the private worker/scanner; integration tests use this boundary.
pub struct FeeBuildTestWorker(ShadowFeeWorker);

impl FeeBuildTestWorker {
    pub fn new(ctx: &'static Context) -> Self {
        use crate::infrastructure::api_trans::{
            AddressLockManager,
            collect_fee::shadow::{ScannerConfig, ShadowScanner},
        };
        use std::sync::Arc;
        let (tx, _) = tokio::sync::mpsc::channel(8);
        let scanner = Arc::new(ShadowScanner::new(ctx, ScannerConfig::default(), tx, None));
        Self(ShadowFeeWorker::new(ctx, Arc::new(AddressLockManager::new()), scanner))
    }

    /// Installed fake adapter consumes the key without signing real transactions.
    pub async fn build(&self, trade_no: &str) -> Result<(), ServiceError> {
        let result =
            self.0.build_fee_candidate(trade_no, Some("mock-private-key".to_string().into())).await;
        if let Err(err) = result {
            self.0.handle_fee_tx_failed(trade_no, err).await?;
        }
        Ok(())
    }

    pub async fn fail(&self, trade_no: &str, error: ServiceError) -> Result<(), ServiceError> {
        self.0.handle_fee_tx_failed(trade_no, error).await
    }

    pub async fn broadcast(&self, trade_no: &str) -> Result<(), ServiceError> {
        self.0
            .handle(
                crate::infrastructure::api_trans::collect_fee::shadow::ShadowFeeCommand::Broadcast(
                    trade_no.to_owned(),
                ),
            )
            .await
    }
}

/// Exercise the existing receipt worker against the integration backend recorder.
pub async fn upload_fee_receipt_via_worker(ctx: &'static Context, trade_no: &str) {
    use crate::infrastructure::api_trans::collect_fee::shadow::{
        ScannerConfig, ShadowScanner, SideEffectCommand, SideEffectWorker,
    };
    let (tx, _) = tokio::sync::mpsc::channel(8);
    let scanner = std::sync::Arc::new(ShadowScanner::new(ctx, ScannerConfig::default(), tx, None));
    SideEffectWorker::new(ctx, scanner)
        .handle(SideEffectCommand::UploadTxExecReceipt(trade_no.to_owned()))
        .await;
}
