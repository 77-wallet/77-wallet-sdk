use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};
use wallet_chain_interact::types::ChainPrivateKey;

use crate::{domain::api_wallet::account::ApiAccountDomain, error::service::ServiceError};

#[async_trait]
trait PrivateKeyFetcher: Send + Sync {
    async fn fetch(&self, address: &str, chain_code: &str)
    -> Result<ChainPrivateKey, ServiceError>;
}

struct DefaultPrivateKeyFetcher {
    ctx: &'static crate::context::Context,
}

#[async_trait]
impl PrivateKeyFetcher for DefaultPrivateKeyFetcher {
    async fn fetch(
        &self,
        address: &str,
        chain_code: &str,
    ) -> Result<ChainPrivateKey, ServiceError> {
        ApiAccountDomain::get_private_key_with_ctx(self.ctx, address, chain_code).await
    }
}

/// 私钥请求消息
#[derive(Debug)]
enum PrivateKeyCmd {
    Get {
        address: String,
        chain_code: String,
        resp: tokio::sync::oneshot::Sender<Result<ChainPrivateKey, ServiceError>>,
    },
    FetchResult {
        address: String,
        chain_code: String,
        result: Result<ChainPrivateKey, ServiceError>,
    },
    Preload {
        address: String,
        chain_code: String,
    },
}

type CacheKey = (String, String);

pub struct PrivateKeyActor {
    rx: mpsc::Receiver<PrivateKeyCmd>,
    tx: mpsc::Sender<PrivateKeyCmd>,
    fetcher: Arc<dyn PrivateKeyFetcher>,
    inflight: HashMap<CacheKey, Vec<oneshot::Sender<Result<ChainPrivateKey, ServiceError>>>>,
}

impl PrivateKeyActor {
    async fn run(mut self) {
        info!("PrivateKeyActor started, beginning to process private key commands");

        while let Some(cmd) = self.rx.recv().await {
            self.handle_cmd(cmd).await;
        }
    }

    async fn handle_cmd(&mut self, cmd: PrivateKeyCmd) {
        match cmd {
            PrivateKeyCmd::Get { address, chain_code, resp } => {
                let key = (address.clone(), chain_code.clone());
                info!(address = %address, chain_code = %chain_code, "Received Get private key command");

                if let Some(waiters) = self.inflight.get_mut(&key) {
                    info!(address = %address, chain_code = %chain_code, "Inflight request found, adding to waiters list");
                    waiters.push(resp);
                    return;
                }

                info!(address = %address, chain_code = %chain_code, "Cache miss, initiating private key retrieval");
                self.inflight.insert(key.clone(), vec![resp]);

                let tx = self.tx.clone();
                let fetcher = self.fetcher.clone();
                tokio::spawn(async move {
                    let result = fetcher.fetch(&address, &chain_code).await;

                    if let Err(err) =
                        tx.send(PrivateKeyCmd::FetchResult { address, chain_code, result }).await
                    {
                        error!("failed to send FetchResult to private key actor: {}", err);
                    }
                });
            }
            PrivateKeyCmd::FetchResult { address, chain_code, result } => {
                let key = (address.clone(), chain_code.clone());
                info!(address = %address, chain_code = %chain_code, "Received FetchResult command");

                if let Some(waiters) = self.inflight.remove(&key) {
                    info!(address = %address, chain_code = %chain_code, waiters_count = %waiters.len(), "Processing result for inflight requests");
                    match result {
                        Ok(pk) => {
                            for resp in waiters {
                                if resp.send(Ok(pk.clone())).is_err() {
                                    info!(address = %address, chain_code = %chain_code, "waiter dropped before private key delivery");
                                }
                            }
                            info!(address = %address, chain_code = %chain_code, "Distributed private key to all waiters");
                        }
                        Err(err) => {
                            error!(address = %address, chain_code = %chain_code, err = %err, "Failed to get private key");
                            let mut it = waiters.into_iter();
                            if let Some(last) = it.next_back() {
                                for resp in it {
                                    if resp
                                        .send(Err(ServiceError::System(
                                            crate::error::system::SystemError::Internal(
                                                err.to_string(),
                                            ),
                                        )))
                                        .is_err()
                                    {
                                        info!(address = %address, chain_code = %chain_code, "waiter dropped before private key error delivery");
                                    }
                                }
                                if last.send(Err(err)).is_err() {
                                    info!(address = %address, chain_code = %chain_code, "last waiter dropped before private key error delivery");
                                }
                            }
                            info!(address = %address, chain_code = %chain_code, "Distributed error to all waiters");
                        }
                    }
                }
            }
            PrivateKeyCmd::Preload { address, chain_code } => {
                let fetcher = self.fetcher.clone();
                info!(address = %address, chain_code = %chain_code, "Received Preload private key command");

                tokio::spawn(async move {
                    info!(address = %address, chain_code = %chain_code, "Starting async preload task");
                    match fetcher.fetch(&address, &chain_code).await {
                        Ok(_) => {
                            info!(address = %address, chain_code = %chain_code, "Preload completed successfully");
                        }
                        Err(e) => {
                            error!(address = %address, chain_code = %chain_code, err = %e, "Preload failed to get private key");
                        }
                    }
                });
            }
        }
    }
}

/// 基于 channel 的私钥管理器
#[derive(Debug, Clone)]
pub struct PrivateKeyManager {
    tx: mpsc::Sender<PrivateKeyCmd>,
}

impl PrivateKeyManager {
    /// 创建新的私钥管理器实例
    pub fn start(ctx: &'static crate::context::Context) -> Self {
        Self::start_with_fetcher(Arc::new(DefaultPrivateKeyFetcher { ctx }))
    }

    #[cfg(test)]
    fn start_with_fetcher(fetcher: Arc<dyn PrivateKeyFetcher>) -> Self {
        info!("Creating new PrivateKeyManager instance");
        let (tx, rx) = mpsc::channel(128);

        let actor = PrivateKeyActor { rx, tx: tx.clone(), fetcher, inflight: HashMap::new() };
        tokio::spawn(actor.run());

        info!("PrivateKeyManager started successfully");
        Self { tx }
    }

    #[cfg(not(test))]
    fn start_with_fetcher(fetcher: Arc<dyn PrivateKeyFetcher>) -> Self {
        info!("Creating new PrivateKeyManager instance");
        let (tx, rx) = mpsc::channel(128);

        let actor = PrivateKeyActor { rx, tx: tx.clone(), fetcher, inflight: HashMap::new() };
        tokio::spawn(actor.run());

        info!("PrivateKeyManager started successfully");
        Self { tx }
    }

    /// 获取私钥
    pub async fn get_private_key(
        &self,
        address: &str,
        chain_code: &str,
    ) -> Result<ChainPrivateKey, ServiceError> {
        info!(address = %address, chain_code = %chain_code, "Public API: get_private_key called");
        let (resp_tx, resp_rx) = oneshot::channel();

        self.tx
            .send(PrivateKeyCmd::Get {
                address: address.to_string(),
                chain_code: chain_code.to_string(),
                resp: resp_tx,
            })
            .await
            .map_err(|_| {
                error!(address = %address, chain_code = %chain_code, "Failed to send Get command to PrivateKeyActor");
                ServiceError::System(crate::error::system::SystemError::Internal(
                    "PrivateKeyActor not running".into(),
                ))
            })?;

        let result = resp_rx.await.map_err(|_| {
            error!(address = %address, chain_code = %chain_code, "PrivateKeyActor dropped response channel");
            ServiceError::System(crate::error::system::SystemError::Internal(
                "PrivateKeyActor dropped response".into(),
            ))
        })?;

        match &result {
            Ok(_) => {
                info!(address = %address, chain_code = %chain_code, "Public API: get_private_key completed successfully")
            }
            Err(err) => {
                error!(address = %address, chain_code = %chain_code, err = %err, "Public API: get_private_key failed")
            }
        }

        result
    }

    /// 预加载私钥，仅触发临时派生，不做结果缓存
    pub async fn preload(&self, address: &str, chain_code: &str) -> Result<(), ServiceError> {
        info!(address = %address, chain_code = %chain_code, "Public API: preload called");
        let message = PrivateKeyCmd::Preload {
            address: address.to_string(),
            chain_code: chain_code.to_string(),
        };

        if let Err(e) = self.tx.send(message).await {
            error!(address = %address, chain_code = %chain_code, err = %e, "Failed to send Preload command to PrivateKeyActor");
            return Err(ServiceError::System(crate::error::system::SystemError::Internal(
                format!("Failed to store private key request: {:?}", e),
            )));
        }
        info!(address = %address, chain_code = %chain_code, "Public API: preload command sent successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::{PrivateKeyFetcher, PrivateKeyManager};
    use async_trait::async_trait;
    use tokio::sync::Semaphore;
    use wallet_chain_interact::types::ChainPrivateKey;

    struct BlockingFetcher {
        calls: AtomicUsize,
        completions: AtomicUsize,
        release: Arc<Semaphore>,
        key: ChainPrivateKey,
    }

    impl BlockingFetcher {
        fn new(key: ChainPrivateKey) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                completions: AtomicUsize::new(0),
                release: Arc::new(Semaphore::new(0)),
                key,
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn completions(&self) -> usize {
            self.completions.load(Ordering::SeqCst)
        }

        fn release(&self) {
            self.release.add_permits(1);
        }
    }

    #[async_trait]
    impl PrivateKeyFetcher for BlockingFetcher {
        async fn fetch(
            &self,
            _address: &str,
            _chain_code: &str,
        ) -> Result<ChainPrivateKey, crate::error::service::ServiceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let permit = self.release.acquire().await.unwrap();
            drop(permit);
            self.completions.fetch_add(1, Ordering::SeqCst);
            Ok(self.key.clone())
        }
    }

    async fn wait_for_value(counter: &AtomicUsize, expected: usize) {
        for _ in 0..100 {
            if counter.load(Ordering::SeqCst) == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(counter.load(Ordering::SeqCst), expected);
    }

    #[tokio::test]
    async fn get_private_key_dedups_inflight() {
        let fetcher_impl = BlockingFetcher::new("mock-private-key".to_string().into());
        let fetcher: Arc<dyn PrivateKeyFetcher> = fetcher_impl.clone();
        let manager = PrivateKeyManager::start_with_fetcher(fetcher);

        let first = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.get_private_key("addr-1", "eth").await })
        };
        let second = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.get_private_key("addr-1", "eth").await })
        };

        wait_for_value(&fetcher_impl.calls, 1).await;
        fetcher_impl.release();
        wait_for_value(&fetcher_impl.completions, 1).await;

        let first = first.await.expect("first join").expect("first result");
        let second = second.await.expect("second join").expect("second result");

        assert_eq!(&*first, &*second);
        assert_eq!(&*first, "mock-private-key");
        assert_eq!(fetcher_impl.calls(), 1);
        assert_eq!(fetcher_impl.completions(), 1);
    }

    #[tokio::test]
    async fn preload_does_not_persist_private_key() {
        let fetcher_impl = BlockingFetcher::new("mock-private-key".to_string().into());
        let fetcher: Arc<dyn PrivateKeyFetcher> = fetcher_impl.clone();
        let manager = PrivateKeyManager::start_with_fetcher(fetcher);

        manager.preload("addr-1", "eth").await.expect("preload request");
        wait_for_value(&fetcher_impl.calls, 1).await;
        fetcher_impl.release();
        wait_for_value(&fetcher_impl.completions, 1).await;

        let key = manager
            .get_private_key("addr-1", "eth")
            .await
            .expect("private key should be derived again after preload");

        assert_eq!(&*key, "mock-private-key");
        assert_eq!(fetcher_impl.calls(), 2);
        assert_eq!(fetcher_impl.completions(), 2);
    }
}
