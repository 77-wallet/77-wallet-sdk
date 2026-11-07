use crate::{
    context::Context,
    error::{service::ServiceError, system::SystemError},
};
use alloy::primitives::U256;
use std::any::Any;
use wallet_chain_interact::{
    tron::protocol::account::AccountResourceDetail,
    types::{FetchMultisigAddressResp, MultisigSignResp, MultisigTxResp},
};
use wallet_utils::{conversion, unit};

use crate::{
    domain::chain::TransferResp,
    // infrastructure::swap_client::AggQuoteResp, // AggQuoteResp未使用
    request::{
        api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
        // transaction::{ApproveReq, DepositReq, QuoteReq, SwapReq, WithdrawReq}, // 未使用的transaction类型
    },
    response_vo::{MultisigQueueFeeParams, TransferParams},
};

use wallet_chain_interact::types::ChainPrivateKey;

use wallet_database::entities::{api_coin::ApiCoinEntity, asset_token_key::AssetTokenKey};

use wallet_database::entities::{
    api_assets::ApiAssetsEntity, multisig_account::MultisigAccountEntity,
    multisig_member::MultisigMemberEntities, multisig_queue::MultisigQueueEntity,
    permission::PermissionEntity,
};
use wallet_transport_backend::{api::BackendApi, response_vo::chain::GasOracle};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum RawTx {
    Tron(
        wallet_chain_interact::tron::operations::RawTransactionParams,
        wallet_chain_interact::BillResourceConsume,
        String,
    ),
    Evm(Vec<u8>, U256),  // eth/bnb/polygon
    Sol(String, String), // solana tx serialized
}

/// Verified canonical included result of a linked replacement competition.
/// Uses normal collect receipt semantics; it is not an L1-finality proof.
#[derive(Debug, Clone)]
pub struct ReplacementReceipt {
    pub transaction_fee: String,
    pub resource_consume: String,
    pub tx_hash: String,
    pub success: bool,
    pub block_hash: String,
    pub block_number: u64,
    pub transaction_time: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TxVisibilityProbe {
    pub signature_status: Option<String>,
    pub transaction_result: Option<String>,
    pub health: Option<String>,
    pub slot: Option<u64>,
    pub visibility_reason: Option<String>,
    pub seen_on_node: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeEstimateSnapshot {
    pub amount: String,
    pub energy: Option<u64>,
    pub bandwidth: Option<u64>,
}

impl FeeEstimateSnapshot {
    pub fn from_response(response: &str) -> Result<Self, ServiceError> {
        let value: serde_json::Value = wallet_utils::serde_func::serde_from_str(response)?;
        let estimate_fee = Self::select_estimate_fee(&value)?;
        let amount = Self::parse_amount(estimate_fee)?;
        Ok(Self { amount, energy: None, bandwidth: None })
    }

    pub fn with_tron_resources(mut self, energy: u64, bandwidth: u64) -> Self {
        self.energy = Some(energy);
        self.bandwidth = Some(bandwidth);
        self
    }

    fn select_estimate_fee(value: &serde_json::Value) -> Result<&serde_json::Value, ServiceError> {
        if let Some(estimate_fee) = value.get("estimateFee") {
            return Ok(estimate_fee);
        }

        let options = value
            .get("data")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Self::invalid_response("missing data"))?;
        let default_type = value.get("default").and_then(serde_json::Value::as_str);
        let selected = default_type
            .and_then(|kind| {
                options.iter().find(|option| {
                    option.get("type").and_then(serde_json::Value::as_str) == Some(kind)
                })
            })
            .or_else(|| options.first())
            .ok_or_else(|| Self::invalid_response("contains no fee options"))?;

        selected.get("estimateFee").ok_or_else(|| Self::invalid_response("missing estimateFee"))
    }

    fn parse_amount(estimate_fee: &serde_json::Value) -> Result<String, ServiceError> {
        let amount =
            estimate_fee.get("amount").ok_or_else(|| Self::invalid_response("missing amount"))?;
        let display_amount = amount
            .as_str()
            .map(str::to_string)
            .or_else(|| amount.as_f64().map(Self::format_f64))
            .ok_or_else(|| Self::invalid_response("amount is not numeric"))?;
        let amount = conversion::decimal_from_str(&display_amount)?;
        Ok(amount.normalize().to_string())
    }

    fn format_f64(amount: f64) -> String {
        let text = format!("{amount:.8}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text == "-0" { "0".to_string() } else { text.to_string() }
    }

    fn invalid_response(reason: &str) -> ServiceError {
        ServiceError::System(SystemError::Internal(format!("fee estimate response {reason}")))
    }
}

#[async_trait::async_trait]
pub trait Oracle {
    async fn gas_oracle(&self) -> Result<GasOracle, crate::error::service::ServiceError>;

    async fn default_gas_oracle(&self) -> Result<GasOracle, crate::error::service::ServiceError>;

    async fn gas_oracle_with_ctx(
        &self,
        _ctx: &Context,
    ) -> Result<GasOracle, crate::error::service::ServiceError> {
        self.gas_oracle().await
    }
}

#[async_trait::async_trait]
pub trait Tx: Any {
    fn rpc_endpoint_for_log(&self) -> Option<String> {
        None
    }

    fn check_min_transfer(
        &self,
        value: &str,
        decimal: u8,
    ) -> Result<U256, crate::error::service::ServiceError> {
        let min = U256::from(1);
        let transfer_amount = unit::convert_to_u256(value, decimal)?;

        if transfer_amount < min {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::AmountLessThanMin,
            ))?;
        }
        Ok(transfer_amount)
    }

    async fn account_resource(
        &self,
        owner_address: &str,
    ) -> Result<AccountResourceDetail, crate::error::service::ServiceError>;

    async fn balance_token_key(
        &self,
        addr: &str,
        token: AssetTokenKey,
    ) -> Result<U256, wallet_chain_interact::Error>;

    async fn balance(
        &self,
        addr: &str,
        token: Option<String>,
    ) -> Result<U256, wallet_chain_interact::Error> {
        self.balance_token_key(addr, AssetTokenKey::from_raw(token.as_deref())).await
    }

    async fn nonce(&self, addr: &str) -> Result<u64, crate::error::service::ServiceError>;

    async fn block_num(&self) -> Result<u64, wallet_chain_interact::Error>;

    async fn query_tx_res(
        &self,
        hash: &str,
    ) -> Result<Option<wallet_chain_interact::QueryTransactionResult>, wallet_chain_interact::Error>;

    /// Return evidence only for a different transaction included in a finalized block.
    /// Unsupported chains/providers must leave the original transaction uncertain.
    async fn finalized_nonce_conflict(
        &self,
        _chain_code: &str,
        _address: &str,
        _nonce: i32,
        _original_hash: &str,
    ) -> Result<
        Option<wallet_database::repositories::api_wallet::collect::FinalizedCollectNonceConflict>,
        crate::error::service::ServiceError,
    > {
        Ok(None)
    }

    /// Confirmed frontier for explicit same-nonce recovery; never fall back to pending.
    async fn replacement_latest_nonce(&self, _address: &str) -> Result<u64, ServiceError> {
        Err(ServiceError::Parameter("latest nonce unsupported for scoped replacement".into()))
    }

    /// Confirm a known signed identity using the normal included-receipt policy.
    /// Missing/uncertain receipts and pending nonce observations are not evidence.
    async fn replacement_receipt(
        &self,
        _hash: &str,
        _address: &str,
        _nonce: u64,
    ) -> Result<Option<ReplacementReceipt>, ServiceError> {
        Ok(None)
    }

    /// Probe the node for Solana-like visibility signals.
    ///
    /// Default implementation returns `None` so non-Solana chains keep the
    /// existing behavior without extra RPC calls.
    async fn tx_visibility_probe(
        &self,
        _hash: &str,
    ) -> Result<Option<TxVisibilityProbe>, ServiceError> {
        Ok(None)
    }

    /// 查询交易是否已被当前节点看到（不要求确认）。
    ///
    /// 默认返回 `false`，由链适配器按需覆盖（当前 EVM / SOL 使用）。
    async fn query_tx_seen_on_node(&self, hash: &str) -> Result<bool, ServiceError> {
        Ok(self.tx_visibility_probe(hash).await?.is_some_and(|probe| probe.seen_on_node))
    }

    /// 查询交易是否仍在 pending pool 中。
    ///
    /// 默认返回 `false`，由链适配器按需覆盖。
    async fn has_pending_tx(&self, _hash: &str) -> Result<bool, ServiceError> {
        Ok(false)
    }

    async fn token_symbol(&self, token: &str) -> Result<String, wallet_chain_interact::Error>;

    async fn token_name(&self, token: &str) -> Result<String, wallet_chain_interact::Error>;

    async fn decimals(&self, token: &str) -> Result<u8, wallet_chain_interact::Error>;

    async fn black_address(
        &self,
        token: &str,
        owner: &str,
    ) -> Result<bool, crate::error::service::ServiceError>;

    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, crate::error::service::ServiceError>;

    async fn transfer_with_ctx(
        &self,
        _ctx: &Context,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        self.transfer(params, private_key).await
    }

    async fn estimate_fee(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError>;

    async fn estimate_fee_with_ctx(
        &self,
        _ctx: &Context,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        self.estimate_fee(req, main_symbol).await
    }

    /// Exact native fee reserve for automatic EVM order funding, in wei.
    /// Presentation responses must never be used to derive this amount.
    async fn estimate_native_fee_wei(
        &self,
        _req: ApiBaseTransferReq,
    ) -> Result<U256, ServiceError> {
        Err(ServiceError::Parameter("exact native fee estimate is unsupported".to_string()))
    }

    /// Estimate a fee without applying local balance gates.
    ///
    /// This is reserved for notification/reporting flows that only need the
    /// fee amount fact, not a final spendability verdict.
    async fn estimate_fee_without_balance_check(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        self.estimate_fee(req, main_symbol).await
    }

    async fn estimate_fee_snapshot(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<FeeEstimateSnapshot, ServiceError> {
        let response = self.estimate_fee_without_balance_check(req, main_symbol).await?;
        FeeEstimateSnapshot::from_response(&response)
    }

    /// Return the lamports that must be reserved for the recipient token ATA,
    /// if the chain needs to account for that rent explicitly.
    ///
    /// Default implementation keeps non-Solana adapters unchanged.
    async fn recipient_ata_rent(
        &self,
        _req: &ApiBaseTransferReq,
    ) -> Result<u64, crate::error::service::ServiceError> {
        Ok(0)
    }

    async fn build_transfer_raw(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<(String, RawTx, String), crate::error::service::ServiceError>;

    async fn broadcast_transfer(
        &self,
        raw: RawTx,
    ) -> Result<TransferResp, crate::error::service::ServiceError>;

    // async fn approve(
    //     &self,
    //     req: &ApproveReq,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, crate::error::service::ServiceError>;
    //
    // async fn approve_fee(
    //     &self,
    //     req: &ApproveReq,
    //     value: U256,
    //     main_symbol: &str,
    // ) -> Result<String, crate::error::service::ServiceError>;
    //
    // async fn allowance(
    //     &self,
    //     from: &str,
    //     token: &str,
    //     spender: &str,
    // ) -> Result<U256, crate::error::service::ServiceError>;
    //
    // async fn swap_quote(
    //     &self,
    //     req: &QuoteReq,
    //     quote_resp: &AggQuoteResp,
    //     symbol: &str,
    // ) -> Result<(U256, String, String), crate::error::service::ServiceError>;
    //
    // async fn swap(
    //     &self,
    //     req: &SwapReq,
    //     fee: String,
    //     key: ChainPrivateKey,
    // ) -> Result<TransferResp, crate::error::service::ServiceError>;
    //
    // async fn deposit_fee(
    //     &self,
    //     req: DepositReq,
    //     main_coin: &CoinEntity,
    // ) -> Result<(String, String), crate::error::service::ServiceError>;
    //
    // async fn deposit(
    //     &self,
    //     req: &DepositReq,
    //     fee: String,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, crate::error::service::ServiceError>;
    //
    // async fn withdraw_fee(
    //     &self,
    //     req: WithdrawReq,
    //     main_coin: &CoinEntity,
    // ) -> Result<(String, String), crate::error::service::ServiceError>;
    //
    // async fn withdraw(
    //     &self,
    //     req: &WithdrawReq,
    //     fee: String,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, crate::error::service::ServiceError>;
}

#[async_trait::async_trait]
pub trait Multisig {
    async fn multisig_address(
        &self,
        account: &MultisigAccountEntity,
        member: &MultisigMemberEntities,
    ) -> Result<FetchMultisigAddressResp, crate::error::service::ServiceError>;

    async fn deploy_multisig_account(
        &self,
        account: &MultisigAccountEntity,
        member: &MultisigMemberEntities,
        fee_setting: Option<String>,
        key: ChainPrivateKey,
    ) -> Result<(String, String), crate::error::service::ServiceError>;

    async fn deploy_multisig_fee(
        &self,
        account: &MultisigAccountEntity,
        member: MultisigMemberEntities,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError>;

    async fn build_multisig_fee(
        &self,
        req: &MultisigQueueFeeParams,
        account: &MultisigAccountEntity,
        decimal: u8,
        token: Option<String>,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError>;

    async fn build_multisig_with_account(
        &self,
        req: &TransferParams,
        account: &MultisigAccountEntity,
        assets: &ApiAssetsEntity,
        key: ChainPrivateKey,
    ) -> Result<MultisigTxResp, crate::error::service::ServiceError>;

    async fn build_multisig_with_permission(
        &self,
        req: &TransferParams,
        p: &PermissionEntity,
        coin: &ApiCoinEntity,
    ) -> Result<MultisigTxResp, crate::error::service::ServiceError>;

    async fn sign_fee(
        &self,
        account: &MultisigAccountEntity,
        address: &str,
        raw_data: &str,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError>;

    async fn sign_multisig_tx(
        &self,
        account: &MultisigAccountEntity,
        address: &str,
        key: ChainPrivateKey,
        raw_data: &str,
    ) -> Result<MultisigSignResp, crate::error::service::ServiceError>;

    async fn estimate_multisig_fee(
        &self,
        queue: &MultisigQueueEntity,
        coin: &ApiCoinEntity,
        backend: &BackendApi,
        sign_list: Vec<String>,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError>;
}

// 创建一个枚举来包装所有 Tx 实现
// #[enum_dispatch::enum_dispatch(Tx)]
// pub enum ApiTxAdapter {
//     Btc(BtcTx),
//     Doge(DogeTx),
//     Eth(EthTx),
//     Bnb(EthTx),
//     Ltc(LtcTx),
//     Sol(SolTx),
//     Sui(SuiTx),
//     Ton(TonTx),
//     Tron(TronTx),
// }

#[cfg(test)]
mod tests {
    use super::FeeEstimateSnapshot;

    #[test]
    fn fee_estimate_snapshot_parses_default_option() {
        let response = r#"{
            "default":"propose",
            "data":[
                {"type":"safe","estimateFee":{"amount":"0.00001"}},
                {"type":"propose","estimateFee":{"amount":"0.00003313"}}
            ]
        }"#;

        let snapshot = FeeEstimateSnapshot::from_response(response).expect("parse fee estimate");
        assert_eq!(snapshot.amount, "0.00003313");
        assert_eq!(snapshot.energy, None);
        assert_eq!(snapshot.bandwidth, None);
    }

    #[test]
    fn fee_estimate_snapshot_parses_top_level_amount() {
        let response = r#"{"estimateFee":{"amount":"0.000005"}}"#;

        let snapshot = FeeEstimateSnapshot::from_response(response).expect("parse fee estimate");
        assert_eq!(snapshot.amount, "0.000005");
    }

    #[test]
    fn fee_estimate_snapshot_formats_numeric_amount() {
        let response = r#"{"estimateFee":{"amount":0.12340000}}"#;

        let snapshot = FeeEstimateSnapshot::from_response(response).expect("parse fee estimate");
        assert_eq!(snapshot.amount, "0.1234");
    }
}
