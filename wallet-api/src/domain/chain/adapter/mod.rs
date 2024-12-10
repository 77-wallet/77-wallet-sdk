mod transaction_adapter;
pub use transaction_adapter::*;
mod multisig_adapter;
use super::rpc_need_header;
pub use multisig_adapter::*;
use wallet_database::entities::chain::{ChainEntity, ChainWithNode};

#[macro_export]
macro_rules! dispatch {
    ($self:expr, $method:ident, $($arg:expr),*) => {
        match $self {
            Self::BitCoin(chain) => chain.$method($($arg),*).await,
            Self::Ethereum(chain) => chain.$method($($arg),*).await,
            Self::Solana(chain) => chain.$method($($arg),*).await,
            Self::Tron(chain) => chain.$method($($arg),*).await,
        }
    };
}

pub struct ChainAdapterFactory;
impl ChainAdapterFactory {
    async fn get_chain_node(chain_code: &str) -> Result<ChainWithNode, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let node = ChainEntity::chain_node_info(pool.as_ref(), chain_code)
            .await?
            .ok_or(crate::BusinessError::Chain(crate::ChainError::NotFound(
                chain_code.to_string(),
            )))?;
        Ok(node)
    }

    pub async fn get_multisig_adapter(
        chain_code: &str,
    ) -> Result<MultisigAdapter, crate::ServiceError> {
        let node = ChainAdapterFactory::get_chain_node(chain_code).await?;

        let chain = wallet_types::chain::chain::ChainCode::try_from(node.chain_code.as_str())?;

        let header_opt = if rpc_need_header(&node.rpc_url)? {
            Some(crate::Context::get_rpc_header().await?)
        } else {
            None
        };

        MultisigAdapter::new(chain, node, header_opt)
    }

    pub async fn get_transaction_adapter(
        chain_code: &str,
    ) -> Result<TransactionAdapter, crate::ServiceError> {
        let node = ChainAdapterFactory::get_chain_node(chain_code).await?;
        let chain = wallet_types::chain::chain::ChainCode::try_from(node.chain_code.as_str())?;

        let header_opt = if rpc_need_header(&node.rpc_url)? {
            Some(crate::Context::get_rpc_header().await?)
        } else {
            None
        };

        Ok(TransactionAdapter::new(
            chain,
            &node.rpc_url,
            &node.http_url,
            header_opt,
        )?)
    }

    pub async fn get_node_transaction_adapter(
        chain_code: &str,
        rpc_url: &str,
        http_url: &str,
    ) -> Result<TransactionAdapter, crate::ServiceError> {
        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code)?;

        let header_opt = if rpc_need_header(rpc_url)? {
            Some(crate::Context::get_rpc_header().await?)
        } else {
            None
        };

        Ok(TransactionAdapter::new(
            chain, rpc_url, http_url, header_opt,
        )?)
    }
}
