mod evm_fee;
pub(crate) mod evm_submission;
mod transaction_adapter;
use crate::context::Context;
pub use transaction_adapter::*;
use wallet_chain_interact::{
    tron::{self, TronChain},
    types::ChainPrivateKey,
};
use wallet_transport::client::HttpClient;
use wallet_types::constant::chain_code;
mod multisig_adapter;
use crate::request::transaction::TransferReq;

use super::rpc_need_header;
pub use multisig_adapter::*;
pub mod eth;
pub mod eth_tx;
pub mod sol_tx;
pub mod ton_tx;
pub mod tron_tx;

use wallet_database::entities::chain::ChainWithNode;

const TIME_OUT: u64 = 30;

#[macro_export]
macro_rules! dispatch {
    ($self:expr, $method:ident, $($arg:expr),*) => {
        match $self {
            Self::BitCoin(chain) => chain.$method($($arg),*).await,
            Self::Ethereum(chain) => chain.$method($($arg),*).await,
            Self::Solana(chain) => chain.$method($($arg),*).await,
            Self::Tron(chain) => chain.$method($($arg),*).await,
            Self::Ltc(chain) => chain.$method($($arg),*).await,
            Self::Doge(chain) => chain.$method($($arg),*).await,
            Self::Ton(chain) => chain.$method($($arg),*).await,
            Self::Sui(chain) => chain.$method($($arg),*).await,
        }
    };
}

pub struct ChainAdapterFactory;
impl ChainAdapterFactory {
    pub(crate) async fn get_chain_node_with_ctx(
        ctx: &Context,
        chain_code: &str,
    ) -> Result<ChainWithNode, crate::error::service::ServiceError> {
        use crate::infrastructure::chain_node::chain_node_ensurer::ChainNodeEnsurer;

        let core_pool = ctx.core_pool()?;
        let api_pool = ctx.api_wallet_pool()?;
        let ensurer = ChainNodeEnsurer::new(core_pool, api_pool);

        let chain_with_node =
            ensurer.ensure_and_get_standard_chain_node_with_node(chain_code).await?;

        Ok(chain_with_node.into())
    }

    pub async fn get_multisig_adapter(
        ctx: &Context,
        chain_code: &str,
    ) -> Result<MultisigAdapter, crate::error::service::ServiceError> {
        Self::get_multisig_adapter_with_ctx(ctx, chain_code).await
    }

    pub async fn get_multisig_adapter_with_ctx(
        ctx: &Context,
        chain_code: &str,
    ) -> Result<MultisigAdapter, crate::error::service::ServiceError> {
        let node = ChainAdapterFactory::get_chain_node_with_ctx(ctx, chain_code).await?;

        let chain = wallet_types::chain::chain::ChainCode::try_from(node.chain_code.as_str())?;

        let header_opt =
            if rpc_need_header(&node.rpc_url)? { Some(ctx.get_rpc_header().await?) } else { None };

        MultisigAdapter::new(chain, node, header_opt)
    }

    pub async fn get_transaction_adapter(
        ctx: &Context,
        chain_code: &str,
    ) -> Result<TransactionAdapter, crate::error::service::ServiceError> {
        Self::get_transaction_adapter_with_ctx(ctx, chain_code).await
    }

    pub async fn get_transaction_adapter_with_ctx(
        ctx: &Context,
        chain_code: &str,
    ) -> Result<TransactionAdapter, crate::error::service::ServiceError> {
        let node = ChainAdapterFactory::get_chain_node_with_ctx(ctx, chain_code).await?;
        let chain = wallet_types::chain::chain::ChainCode::try_from(node.chain_code.as_str())?;
        let network =
            crate::domain::chain::ChainDomain::network_kind_from_node_network(&node.network);

        let header_opt =
            if rpc_need_header(&node.rpc_url)? { Some(ctx.get_rpc_header().await?) } else { None };

        Ok(TransactionAdapter::new(chain, &node.rpc_url, header_opt, network)?)
    }

    pub async fn get_tron_adapter(
        ctx: &Context,
    ) -> Result<TronChain, crate::error::service::ServiceError> {
        Self::get_tron_adapter_with_ctx(ctx).await
    }

    pub async fn get_tron_adapter_with_ctx(
        ctx: &Context,
    ) -> Result<TronChain, crate::error::service::ServiceError> {
        let node = ChainAdapterFactory::get_chain_node_with_ctx(ctx, chain_code::TRON).await?;

        let header_opt =
            if rpc_need_header(&node.rpc_url)? { Some(ctx.get_rpc_header().await?) } else { None };
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));

        let http_client = HttpClient::new(&node.rpc_url, header_opt, timeout)?;
        let provider = tron::Provider::new(http_client)?;

        Ok(tron::TronChain::new(provider)?)
    }

    pub async fn get_node_transaction_adapter(
        ctx: &Context,
        chain_code: &str,
        rpc_url: &str,
        network: &str,
    ) -> Result<TransactionAdapter, crate::error::service::ServiceError> {
        Self::get_node_transaction_adapter_with_ctx(ctx, chain_code, rpc_url, network).await
    }

    pub async fn get_node_transaction_adapter_with_ctx(
        ctx: &Context,
        chain_code: &str,
        rpc_url: &str,
        network: &str,
    ) -> Result<TransactionAdapter, crate::error::service::ServiceError> {
        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code)?;
        let network = crate::domain::chain::ChainDomain::network_kind_from_node_network(network);

        let header_opt =
            if rpc_need_header(rpc_url)? { Some(ctx.get_rpc_header().await?) } else { None };

        Ok(TransactionAdapter::new(chain, rpc_url, header_opt, network)?)
    }
}

// transfer estimate fee t
#[async_trait::async_trait]
pub trait ChainAction {
    type Provider;
    type FeeInfo;

    // 获取 gas 估算
    async fn estimate_fee(
        &self,
        provider: Self::FeeInfo,
    ) -> Result<Self::FeeInfo, crate::error::service::ServiceError>;

    // 执行操作（包含签名、广播）
    async fn execute(
        &self,
        fee: String,
        key: ChainPrivateKey,
    ) -> Result<TransferReq, crate::error::service::ServiceError>;
}
