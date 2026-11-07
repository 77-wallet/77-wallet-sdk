use crate::{
    context::Context,
    domain::{
        api_wallet::adapter::{
            btc_tx::BtcTx, doge_tx::DogeTx, eth_tx::EthTx, ltx_tx::LtcTx, sol_tx::SolTx,
            sui_tx::SuiTx, ton_tx::TonTx, tron_tx::TronTx, tx::Tx,
        },
        chain::rpc_need_header,
    },
    error::service::ServiceError,
};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::{
    any::Any,
    sync::Arc,
    time::{Duration, SystemTime},
};
use wallet_database::{entities::chain::ChainWithNode, repositories::node::NodeRepo};
use wallet_types::chain::chain::ChainCode;

// 包装适配器和创建时间的结构体
struct AdapterEntry {
    adapter: Arc<dyn Tx + Send + Sync>,
    created_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterKind {
    Tron,
    Bitcoin,
    Solana,
    Evm,
    Litecoin,
    Dogecoin,
    Sui,
    Ton,
}

fn adapter_kind(chain_code: ChainCode) -> Option<AdapterKind> {
    if crate::domain::chain::is_evm_chain(chain_code) {
        return Some(AdapterKind::Evm);
    }

    Some(match chain_code {
        ChainCode::Tron => AdapterKind::Tron,
        ChainCode::Bitcoin => AdapterKind::Bitcoin,
        ChainCode::Solana => AdapterKind::Solana,
        ChainCode::Litecoin => AdapterKind::Litecoin,
        ChainCode::Dogcoin => AdapterKind::Dogecoin,
        ChainCode::Sui => AdapterKind::Sui,
        ChainCode::Ton => AdapterKind::Ton,
        _ => return None,
    })
}

pub struct ApiChainAdapterFactory {
    transaction_adapter: DashMap<String, AdapterEntry>,
    // 适配器过期时间（不应超过rpc token本地TTL）
    adapter_ttl: Duration,
}

// 创建全局单例
static API_CHAIN_ADAPTER_FACTORY: Lazy<Arc<ApiChainAdapterFactory>> = Lazy::new(|| {
    Arc::new(ApiChainAdapterFactory {
        transaction_adapter: DashMap::new(),
        // 默认适配器过期时间为25分钟（低于rpc token 30分钟TTL）
        adapter_ttl: Duration::from_secs(25 * 60),
    })
});

impl ApiChainAdapterFactory {
    /// 检查适配器是否过期
    fn is_adapter_expired(&self, entry: &AdapterEntry) -> bool {
        SystemTime::now()
            .duration_since(entry.created_at)
            .map(|elapsed| elapsed > self.adapter_ttl)
            .unwrap_or(true)
    }
}

impl ApiChainAdapterFactory {
    /// 获取全局单例实例
    pub fn get_instance() -> Arc<ApiChainAdapterFactory> {
        API_CHAIN_ADAPTER_FACTORY.clone()
    }

    pub fn invalidate_all_cached_adapters() -> usize {
        let factory = Self::get_instance();
        let removed_count = factory.transaction_adapter.len();
        factory.transaction_adapter.clear();
        tracing::warn!(removed_count = removed_count, "invalidated api_wallet cached adapters");
        removed_count
    }

    async fn get_chain_node(
        ctx: &Context,
        chain_code: ChainCode,
    ) -> Result<ChainWithNode, ServiceError> {
        use crate::infrastructure::chain_node::chain_node_ensurer::ChainNodeEnsurer;

        let core_pool = ctx.core_pool()?;
        let api_pool = ctx.api_wallet_pool()?;
        let chain_code_str = chain_code.to_string();
        let ensurer = ChainNodeEnsurer::new(core_pool, api_pool);
        let chain_with_node = ensurer.ensure_and_get_api_chain_with_node(&chain_code_str).await?;
        let chain_with_node: ChainWithNode = chain_with_node.into();
        Ok(chain_with_node)
    }

    /// 预初始化所有链和节点的适配器
    pub async fn pre_init_all_adapters_with_ctx(
        &self,
        ctx: &'static Context,
    ) -> Result<(), ServiceError> {
        // 获取所有节点
        let pool = ctx.core_pool()?;
        let all_nodes = NodeRepo::list(&pool, None).await?;

        tracing::info!(node_count = all_nodes.len(), "开始预初始化所有链和节点的适配器");

        // 为每个节点创建适配器
        for node in all_nodes {
            // 跳过状态为0的节点
            if node.status == 0 {
                continue;
            }

            let chain_code: ChainCode = match node.chain_code.as_str().try_into() {
                Ok(code) => code,
                Err(e) => {
                    tracing::warn!(chain_code=%node.chain_code, error=%e, "无法将链码转换为ChainCode枚举");
                    continue;
                }
            };
            let Some(kind) = adapter_kind(chain_code) else {
                continue;
            };

            // 使用chain_code和rpc_url组合作为缓存键
            let cache_key = format!("{}:{}", chain_code.to_string(), node.rpc_url);

            // 检查是否已经存在于缓存中
            if self.transaction_adapter.contains_key(&cache_key) {
                continue;
            }

            // 创建适配器
            tracing::debug!(chain_code=%chain_code, rpc_url=%node.rpc_url, "预初始化transaction_adapter");

            let header_opt = if rpc_need_header(&node.rpc_url)? {
                Some(ctx.get_rpc_header().await?)
            } else {
                None
            };

            let adapter: Arc<dyn Tx + Send + Sync> = match kind {
                AdapterKind::Tron => {
                    Arc::new(TronTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?)
                }
                AdapterKind::Bitcoin => {
                    Arc::new(BtcTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?)
                }
                AdapterKind::Solana => {
                    Arc::new(SolTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?)
                }
                AdapterKind::Evm => Arc::new(EthTx::new_with_ctx(
                    ctx,
                    chain_code,
                    &node.rpc_url,
                    crate::domain::chain::ChainDomain::network_kind_from_node_network(
                        &node.network,
                    ),
                    header_opt,
                )?),
                AdapterKind::Litecoin => {
                    Arc::new(LtcTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?)
                }
                AdapterKind::Dogecoin => {
                    Arc::new(DogeTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?)
                }
                AdapterKind::Sui => Arc::new(SuiTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
                AdapterKind::Ton => Arc::new(TonTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
            };

            // 存入缓存，包含创建时间
            self.transaction_adapter.insert(
                cache_key,
                AdapterEntry { adapter: adapter.clone(), created_at: SystemTime::now() },
            );
        }

        tracing::info!("所有链和节点的适配器预初始化完成");
        Ok(())
    }

    pub async fn new_transaction_adapter_with_ctx(
        &self,
        ctx: &'static Context,
        chain_code: ChainCode,
    ) -> Result<Arc<dyn Tx + Send + Sync>, ServiceError> {
        let kind = adapter_kind(chain_code).ok_or_else(|| {
            ServiceError::Parameter(format!("unsupported transaction chain: {chain_code}"))
        })?;
        let node = Self::get_chain_node(ctx, chain_code).await?;

        // 使用chain_code和rpc_url组合作为缓存键，确保不同节点的适配器不会被混用
        let cache_key = format!("{}:{}", chain_code.to_string(), node.rpc_url);

        // 尝试从缓存获取
        if let Some(entry) = self.transaction_adapter.get(&cache_key) {
            if !self.is_adapter_expired(&entry) {
                tracing::debug!(chain_code=%chain_code, rpc_url=%node.rpc_url, "使用缓存的transaction_adapter");
                return Ok(entry.adapter.clone());
            } else {
                tracing::info!(chain_code=%chain_code, rpc_url=%node.rpc_url, "缓存的transaction_adapter已过期");
            }
        }

        // 缓存未命中或适配器已过期，创建新的适配器
        tracing::debug!(rpc_url=%node.rpc_url, chain_code=%chain_code, "创建新的transaction_adapter");
        let header_opt =
            if rpc_need_header(&node.rpc_url)? { Some(ctx.get_rpc_header().await?) } else { None };

        let adapter: Arc<dyn Tx + Send + Sync> = match kind {
            AdapterKind::Tron => Arc::new(TronTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
            AdapterKind::Bitcoin => Arc::new(BtcTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
            AdapterKind::Solana => Arc::new(SolTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
            AdapterKind::Evm => Arc::new(EthTx::new_with_ctx(
                ctx,
                chain_code,
                &node.rpc_url,
                crate::domain::chain::ChainDomain::network_kind_from_node_network(&node.network),
                header_opt,
            )?),
            AdapterKind::Litecoin => Arc::new(LtcTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
            AdapterKind::Dogecoin => {
                Arc::new(DogeTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?)
            }
            AdapterKind::Sui => Arc::new(SuiTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
            AdapterKind::Ton => Arc::new(TonTx::new_with_ctx(ctx, &node.rpc_url, header_opt)?),
        };

        // 存入缓存，包含创建时间
        self.transaction_adapter.insert(
            cache_key,
            AdapterEntry { adapter: adapter.clone(), created_at: SystemTime::now() },
        );
        Ok(adapter)
    }

    /// 静态方法，内部调用全局单例
    pub async fn get_transaction_adapter_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
    ) -> Result<Arc<dyn Tx + Send + Sync>, ServiceError> {
        #[cfg(any(test, feature = "integration-tests"))]
        if let Some(adapter) =
            crate::testkit::adapter_factory::maybe_get_transaction_adapter_override(chain_code)
        {
            return Ok(adapter);
        }

        let chain: ChainCode = chain_code.try_into()?;
        let factory = Self::get_instance();
        factory.new_transaction_adapter_with_ctx(ctx, chain).await
    }

    pub(crate) async fn get_tron_adapter_with_ctx(
        ctx: &'static Context,
    ) -> Result<Arc<TronTx>, ServiceError> {
        let adapter = Self::get_transaction_adapter_with_ctx(ctx, "tron").await?;
        let adapter: Arc<dyn Any + Send + Sync> = adapter;
        Arc::downcast::<TronTx>(adapter).map_err(|_| {
            ServiceError::System(crate::error::system::SystemError::Internal(
                "api tron adapter type mismatch".to_string(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AdapterKind, adapter_kind};
    use wallet_types::chain::chain::ChainCode;

    #[test]
    fn adapter_kind_routes_all_evm_chains_to_eth() {
        for chain in [
            ChainCode::Ethereum,
            ChainCode::BnbSmartChain,
            ChainCode::Polygon,
            ChainCode::Avalanche,
            ChainCode::Fantom,
            ChainCode::Arbitrum,
            ChainCode::Base,
            ChainCode::Scroll,
        ] {
            assert_eq!(adapter_kind(chain), Some(AdapterKind::Evm));
        }
    }

    #[test]
    fn adapter_kind_preserves_non_evm_routes() {
        for (chain, expected) in [
            (ChainCode::Tron, AdapterKind::Tron),
            (ChainCode::Bitcoin, AdapterKind::Bitcoin),
            (ChainCode::Solana, AdapterKind::Solana),
            (ChainCode::Litecoin, AdapterKind::Litecoin),
            (ChainCode::Dogcoin, AdapterKind::Dogecoin),
            (ChainCode::Sui, AdapterKind::Sui),
            (ChainCode::Ton, AdapterKind::Ton),
        ] {
            assert_eq!(adapter_kind(chain), Some(expected));
        }
    }
}
