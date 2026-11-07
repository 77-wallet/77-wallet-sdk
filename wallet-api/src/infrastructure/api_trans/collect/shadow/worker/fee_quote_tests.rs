use super::*;
use crate::domain::api_wallet::{RawTx, Tx};
use std::sync::atomic::{AtomicUsize, Ordering};
use wallet_database::entities::asset_token_key::AssetTokenKey;
use wallet_types::chain::chain::ChainCode;

#[derive(Default)]
struct QuoteAdapter {
    balance_wei: U256,
    display_calls: AtomicUsize,
    native_calls: AtomicUsize,
    fail_native: bool,
}
impl QuoteAdapter {
    fn fee_json(&self) -> String {
        serde_json::json!({"default":"propose", "data":[{"type":"propose", "estimateFee":{"amount":"0.000004088","currency":"ETH","unitPrice":0.0,"fiatValue":0.0}, "maxFee":{"amount":"0.000005400503767905","currency":"ETH","unitPrice":0.0,"fiatValue":0.0}, "feeSetting":{"gasLimit":40880,"baseFee":"0","priorityFee":"0","maxFeePerGas":"100000000"}}]}).to_string()
    }
}
fn request(code: ChainCode) -> crate::request::api_wallet::trans::ApiBaseTransferReq {
    let mut req = crate::request::api_wallet::trans::ApiBaseTransferReq::new(
        "from",
        "to",
        "5",
        &code.to_string(),
    );
    req.with_token(Some("0x0000000000000000000000000000000000000001".into()), 6, "USDT");
    req
}
#[async_trait::async_trait]
impl Tx for QuoteAdapter {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<
        wallet_chain_interact::tron::protocol::account::AccountResourceDetail,
        crate::error::service::ServiceError,
    > {
        unimplemented!("not used in collect fee checks")
    }

    async fn balance_token_key(
        &self,
        _addr: &str,
        _token: AssetTokenKey,
    ) -> Result<U256, wallet_chain_interact::Error> {
        Ok(self.balance_wei)
    }

    async fn nonce(&self, _addr: &str) -> Result<u64, crate::error::service::ServiceError> {
        Ok(0)
    }

    async fn block_num(&self) -> Result<u64, wallet_chain_interact::Error> {
        Ok(0)
    }

    async fn query_tx_res(
        &self,
        _hash: &str,
    ) -> Result<Option<wallet_chain_interact::QueryTransactionResult>, wallet_chain_interact::Error>
    {
        Ok(None)
    }

    async fn token_symbol(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("ETH".to_string())
    }

    async fn token_name(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("Ethereum".to_string())
    }

    async fn decimals(&self, _token: &str) -> Result<u8, wallet_chain_interact::Error> {
        Ok(18)
    }

    async fn black_address(
        &self,
        _token: &str,
        _owner: &str,
    ) -> Result<bool, crate::error::service::ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        _params: &crate::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<crate::domain::chain::TransferResp, crate::error::service::ServiceError> {
        unimplemented!("not used in collect fee checks")
    }

    async fn estimate_fee(
        &self,
        _req: crate::request::api_wallet::trans::ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        self.display_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.fee_json())
    }

    async fn estimate_native_fee_wei(
        &self,
        req: crate::request::api_wallet::trans::ApiBaseTransferReq,
    ) -> Result<U256, crate::error::service::ServiceError> {
        self.native_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let replacing = req.metadata.as_deref().is_some_and(|m| m.contains("collectReplacement"));
        assert_eq!(
            req.token_address.as_db_str(),
            if replacing { "" } else { "0x0000000000000000000000000000000000000001" }
        );
        if self.fail_native {
            return Err(crate::error::service::ServiceError::Parameter("L1 RPC failed".into()));
        }
        Ok(U256::from(5_400_503_767_905u64))
    }

    async fn build_transfer_raw(
        &self,
        _params: &crate::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<(String, RawTx, String), crate::error::service::ServiceError> {
        unimplemented!("not used in collect fee checks")
    }

    async fn broadcast_transfer(
        &self,
        _raw: RawTx,
    ) -> Result<crate::domain::chain::TransferResp, crate::error::service::ServiceError> {
        unimplemented!("not used in collect fee checks")
    }
}

#[tokio::test]
async fn fee_quote_base_scroll_includes_l1_instead_of_gas_only_display() {
    for code in [ChainCode::Base, ChainCode::Scroll] {
        let adapter = QuoteAdapter::default();
        let amount = estimate_evm_collect_fee(&adapter, code, request(code), "ETH").await.unwrap();
        assert_eq!(amount, "0.000005400503767905");
        assert_eq!(adapter.native_calls.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.display_calls.load(Ordering::SeqCst), 0);
        assert_eq!(evm_funding_budget(&amount).unwrap(), "0.000006480604521486");
    }
}
#[tokio::test]
async fn fee_quote_l1_error_does_not_fall_back_to_underfunded_execution_quote() {
    for code in [ChainCode::Base, ChainCode::Scroll] {
        let adapter = QuoteAdapter { fail_native: true, ..Default::default() };
        assert!(estimate_evm_collect_fee(&adapter, code, request(code), "ETH").await.is_err());
        assert_eq!(adapter.display_calls.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn fee_quote_arbitrum_preserves_exact_execution_maximum() {
    let adapter = QuoteAdapter::default();
    assert_eq!(
        estimate_evm_collect_fee(
            &adapter,
            ChainCode::Arbitrum,
            request(ChainCode::Arbitrum),
            "ETH"
        )
        .await
        .unwrap(),
        "0.000004088"
    );
    assert_eq!(adapter.native_calls.load(Ordering::SeqCst), 0);
}
#[test]
fn fee_quote_headroom_covers_logged_arbitrum_increase_and_rounds_up_wei() {
    // Base real-adapter regression uses this exact one-time funding budget.
    assert_eq!(evm_funding_budget("0.000000001100492989").unwrap(), "0.000000001320591587");
    assert_eq!(evm_funding_budget("0.0000049441").unwrap(), "0.00000593292");
    assert_eq!(evm_funding_budget("0.000000000000000001").unwrap(), "0.000000000000000002");
}

#[serial_test::serial]
#[tokio::test]
async fn fee_quote_funding_and_build_use_same_l1_inclusive_amount() {
    use crate::{
        infrastructure::api_trans::{AddressLockManager, ShadowAdvancer},
        testkit::adapter_factory::{
            clear_test_transaction_adapter_override, set_test_transaction_adapter_override,
        },
    };
    use std::sync::Arc;
    struct OverrideGuard(String);
    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            clear_test_transaction_adapter_override(&self.0);
        }
    }
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, tx, None));
    let build =
        ShadowCollectWorker::new(ctx, Arc::new(AddressLockManager::new()), advancer.clone());
    let funding = SideEffectWorker::new(ctx, advancer);
    for code in [ChainCode::Base, ChainCode::Scroll] {
        let adapter = Arc::new(QuoteAdapter::default());
        set_test_transaction_adapter_override(&code.to_string(), adapter.clone());
        let _guard = OverrideGuard(code.to_string());
        let token = AssetTokenKey::Contract("0x0000000000000000000000000000000000000001".into());
        let funding_quote = funding
            .estimate_fee("from", "to", "5", code, "USDT", "ETH", token.clone(), 6)
            .await
            .unwrap();
        let build_quote = build
            .estimate_fee("from", "to", "5", code, "USDT", "ETH", token, 6, false)
            .await
            .unwrap();
        assert_eq!(funding_quote, "0.000005400503767905");
        assert_eq!(build_quote, funding_quote);
        assert_eq!(adapter.native_calls.load(Ordering::SeqCst), 2);
        assert_eq!(adapter.display_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn replacement_quote_uses_exact_native_budget_on_every_evm_chain() {
    for chain in [ChainCode::Ethereum, ChainCode::BnbSmartChain, ChainCode::Base, ChainCode::Scroll]
    {
        let adapter = QuoteAdapter::default();
        let mut params = request(chain);
        params.with_token(None, 18, "ETH");
        params.metadata = Some(
            serde_json::json!({"collectReplacement":{
                "old_raw":"synthetic", "old_hash":"synthetic", "from_addr":"from", "nonce":6
            }})
            .to_string(),
        );
        let result = estimate_evm_collect_fee(&adapter, chain, params, "ETH").await.unwrap();
        assert_eq!(adapter.native_calls.load(Ordering::SeqCst), 1, "{chain}");
        assert_eq!(adapter.display_calls.load(Ordering::SeqCst), 0, "{chain}");
        assert!(!result.is_empty());
    }
}
