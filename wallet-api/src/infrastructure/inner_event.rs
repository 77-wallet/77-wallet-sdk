use crate::{
    domain::{api_wallet::assets::ApiAssetsDomain, assets::AssetsDomain},
    error::service::ServiceError,
};
use futures::FutureExt;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use tokio_stream::StreamExt as _;
use wallet_database::entities::asset_token_key::AssetTokenKey;

pub(crate) type InnerEventSender = tokio::sync::mpsc::UnboundedSender<InnerEvent>;

pub(crate) struct SyncAssetsData {
    // pub(crate) uid: String,
    pub(crate) addr_list: Vec<String>,
    pub(crate) chain_code: String,
    pub(crate) token_address: AssetTokenKey,
    pub(crate) retry_count: u32,
    pub(crate) priority: SyncPriority,
}

impl SyncAssetsData {
    pub(crate) fn new_with_token_key(
        addr_list: Vec<String>,
        chain_code: String,
        token_address: AssetTokenKey,
    ) -> Self {
        Self { addr_list, chain_code, token_address, retry_count: 0, priority: SyncPriority::Low }
    }

    pub(crate) fn with_retry_count(mut self, retry_count: u32) -> Self {
        self.retry_count = retry_count;
        self
    }

    pub(crate) fn with_priority(mut self, priority: SyncPriority) -> Self {
        self.priority = priority;
        self
    }

    fn describe(&self) -> String {
        format!(
            "addr_count={}, chain_code={}, token_address={}, retry_count={}, priority={:?}",
            self.addr_list.len(),
            self.chain_code,
            self.token_address,
            self.retry_count,
            self.priority
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncPriority {
    High,
    Low,
}

// 最大重试次数
const MAX_RETRY_COUNT: u32 = 3;
const NORMAL_SYNC_DELAY_SECS: u64 = 5;
const API_HIGH_SYNC_DELAY_SECS: u64 = 1;
const API_LOW_SYNC_DELAY_SECS: u64 = 5;
const LANE_NORMAL: &str = "normal";
const LANE_API_HIGH: &str = "api_high";
const LANE_API_LOW: &str = "api_low";

pub(crate) enum InnerEvent {
    SyncAssets(SyncAssetsData),
    ApiWalletSyncAssets(SyncAssetsData),
}

impl InnerEvent {
    fn describe(&self) -> String {
        match self {
            Self::SyncAssets(data) => format!("SyncAssets({})", data.describe()),
            Self::ApiWalletSyncAssets(data) => format!("ApiWalletSyncAssets({})", data.describe()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AssetKey {
    address: String,
    chain_code: String,
    token_address: AssetTokenKey,
}

impl AssetKey {
    fn from_sync_data(addr: &str, chain_code: &str, token_address: &AssetTokenKey) -> Self {
        Self {
            address: addr.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_address.clone(),
        }
    }
}

struct EventBuffer {
    name: &'static str,
    buffer: Arc<Mutex<HashSet<AssetKey>>>,
    notifier: Arc<Notify>,
}

impl EventBuffer {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            buffer: Arc::new(Mutex::new(HashSet::new())),
            notifier: Arc::new(Notify::new()),
        }
    }

    fn push_assets(&self, data: SyncAssetsData) {
        if data.addr_list.is_empty() {
            tracing::debug!("收到空地址列表，跳过资产同步事件");
            return;
        }

        // 检查重试次数限制
        if data.retry_count >= MAX_RETRY_COUNT {
            tracing::error!(
                "资产同步任务超过最大重试次数，放弃重试: chain_code={}, token_address={}, addr_count={}, retry_count={}",
                data.chain_code,
                data.token_address,
                data.addr_list.len(),
                data.retry_count
            );
            return;
        }

        // 如果是重试任务，记录日志
        if data.retry_count > 0 {
            tracing::warn!(
                "重试资产同步任务: chain_code={}, token_address={}, addr_count={}, retry_count={}/{}",
                data.chain_code,
                data.token_address,
                data.addr_list.len(),
                data.retry_count,
                MAX_RETRY_COUNT
            );
        }

        let data_desc = data.describe();
        let chain_code = data.chain_code.clone();
        let token_address = data.token_address.clone();

        tracing::debug!("{} 资产同步事件入队: {}", self.name, data_desc.as_str());

        let mut buf = self.buffer.lock().unwrap();
        let was_empty = buf.is_empty();
        let mut added_count = 0;

        for addr in data.addr_list {
            let key = AssetKey::from_sync_data(&addr, &chain_code, &token_address);
            if buf.insert(key) {
                added_count += 1;
            }
        }

        tracing::debug!(
            "{} EventBuffer 添加 {} 个资产项，当前缓冲区大小: {}, {}",
            self.name,
            added_count,
            buf.len(),
            data_desc.as_str()
        );

        if was_empty && !buf.is_empty() {
            tracing::debug!("{} 资产同步缓冲区从空变为非空，唤醒 drain 任务", self.name);
            self.notifier.notify_one();
        }
    }

    // async fn wait_and_drain_after_delay(&self, delay_secs: u64) -> Vec<AssetKey> {
    //     self.notifier.notified().await;
    //     tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

    //     let mut buf = self.buffer.lock().unwrap();
    //     buf.drain().collect()
    // }

    async fn wait_and_drain_stream(
        &self,
        delay_secs: u64,
    ) -> impl tokio_stream::Stream<Item = Vec<AssetKey>> + '_ {
        use tokio_stream::{StreamExt, wrappers::IntervalStream};

        tracing::info!("{} 等待第一次资产变更通知...", self.name);
        self.notifier.notified().await;
        tracing::info!("{} 收到资产变更通知，立即执行第一次 drain", self.name);
        // 1. 第一次立即 drain
        let first = {
            let mut buf = self.buffer.lock().unwrap();
            let drained = buf.drain().collect::<Vec<_>>();
            tracing::info!("{} 第一次 drain 获取到 {} 个资产项", self.name, drained.len());
            drained
        };

        // 用 stream 返回：第一次立即返回 → 然后每隔 delay 秒返回一次
        let delay = tokio::time::Duration::from_secs(delay_secs);
        let interval = tokio::time::interval(delay);
        let interval_stream = IntervalStream::new(interval).filter_map(move |_| {
            let mut buf = self.buffer.lock().unwrap();
            let drained = buf.drain().collect::<Vec<_>>();
            if drained.is_empty() {
                tracing::debug!("{} ⏳ 定时检查：无新增资产变更，跳过", self.name);
                None
            } else {
                tracing::info!("{} 🔁 定时检查：drain 到 {} 个资产项", self.name, drained.len());
                Some(drained)
            }
        });

        tokio_stream::once(first).chain(interval_stream)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InnerEventHandle {
    inner_event_sender: InnerEventSender,
}

impl InnerEventHandle {
    fn dispatch_event(
        event: InnerEvent,
        normal_buf: &EventBuffer,
        api_high_buf: &EventBuffer,
        api_low_buf: &EventBuffer,
    ) {
        match event {
            InnerEvent::SyncAssets(data) => {
                normal_buf.push_assets(data);
            }
            InnerEvent::ApiWalletSyncAssets(data) => match data.priority {
                SyncPriority::High => api_high_buf.push_assets(data),
                SyncPriority::Low => api_low_buf.push_assets(data),
            },
        }
    }

    pub(crate) fn new(ctx: &'static crate::context::Context) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InnerEvent>();
        let normal_buffer = Arc::new(EventBuffer::new(LANE_NORMAL));
        let api_high_buffer = Arc::new(EventBuffer::new(LANE_API_HIGH));
        let api_low_buffer = Arc::new(EventBuffer::new(LANE_API_LOW));
        // 接收事件任务
        {
            let normal_buf = Arc::clone(&normal_buffer);
            let api_high_buf = Arc::clone(&api_high_buffer);
            let api_low_buf = Arc::clone(&api_low_buffer);

            tokio::spawn(async move {
                tracing::info!("inner_event receiver started");
                while let Some(event) = rx.recv().await {
                    let event_desc = event.describe();
                    tracing::debug!("inner_event receiver got event: {}", event_desc);

                    let result = std::panic::AssertUnwindSafe(async {
                        Self::dispatch_event(event, &normal_buf, &api_high_buf, &api_low_buf);
                    })
                    .catch_unwind()
                    .await;

                    if let Err(panic) = result {
                        tracing::error!(
                            panic = ?panic,
                            event = %event_desc,
                            "inner_event receiver panicked while handling event"
                        );
                    }
                }
                tracing::warn!("inner_event receiver stopped: channel closed");
            });
        }

        Self::start_sync_loop(
            ctx,
            Arc::clone(&normal_buffer),
            SyncTarget::Assets,
            NORMAL_SYNC_DELAY_SECS,
            LANE_NORMAL,
        );
        Self::start_sync_loop(
            ctx,
            Arc::clone(&api_high_buffer),
            SyncTarget::ApiAssets,
            API_HIGH_SYNC_DELAY_SECS,
            LANE_API_HIGH,
        );
        Self::start_sync_loop(
            ctx,
            Arc::clone(&api_low_buffer),
            SyncTarget::ApiAssets,
            API_LOW_SYNC_DELAY_SECS,
            LANE_API_LOW,
        );

        Self { inner_event_sender: tx }
    }

    pub(crate) fn send(&self, event: InnerEvent) -> Result<(), ServiceError> {
        tracing::debug!("发送 inner_event: {}", event.describe());
        self.inner_event_sender
            .send(event)
            .map_err(|e| crate::error::system::SystemError::ChannelSendFailed(e.to_string()))?;
        Ok(())
    }

    fn start_sync_loop(
        ctx: &'static crate::context::Context,
        buffer: Arc<EventBuffer>,
        target: SyncTarget,
        delay_secs: u64,
        lane: &'static str,
    ) {
        tokio::spawn(async move {
            tracing::info!("inner_event sync loop started: target={:?}, lane={}", target, lane);
            let mut stream = buffer.wait_and_drain_stream(delay_secs).await;

            while let Some(batch) = stream.next().await {
                if batch.is_empty() {
                    tracing::debug!(
                        "inner_event sync loop got empty batch: target={:?}, lane={}",
                        target,
                        lane
                    );
                    continue;
                }

                let batch_len = batch.len();
                let target_for_batch = target.clone();
                let lane_for_batch = lane;
                let result = std::panic::AssertUnwindSafe(async move {
                    // 分组 chain+token_address → (address list, max_retry_count)
                    // 对于来自 EventBuffer 的批量任务，retry_count 总是 0（首次尝试）
                    let mut grouped: HashMap<(String, AssetTokenKey), Vec<String>> = HashMap::new();
                    for key in batch {
                        grouped
                            .entry((key.chain_code.clone(), key.token_address.clone()))
                            .or_default()
                            .push(key.address.clone());
                    }

                    tracing::info!(
                        "开始批量同步资产: target={:?}, lane={}, 分组数量={}, 总地址数={}",
                        target_for_batch,
                        lane_for_batch,
                        grouped.len(),
                        grouped.values().map(|v| v.len()).sum::<usize>()
                    );

                    for ((chain_code, token_address), addr_list) in grouped {
                        tracing::info!(
                            "同步资产批次: target={:?}, lane={}, chain_code={}, token_address={}, addr_count={}",
                            target_for_batch,
                            lane_for_batch,
                            chain_code,
                            token_address,
                            addr_list.len()
                        );

                        // 首次尝试，retry_count = 0
                        if let Err(e) = Self::sync_assets_once(
                            ctx,
                            chain_code.clone(),
                            token_address.clone(),
                            addr_list,
                            target_for_batch.clone(),
                            0,
                        )
                        .await
                        {
                            tracing::error!(
                                "{:?} sync error: lane={}, chain_code={}, token_address={:?}, error={}",
                                target_for_batch,
                                lane_for_batch,
                                chain_code,
                                token_address,
                                e
                            );
                        }
                    }
                })
                .catch_unwind()
                .await;

                match result {
                    Ok(_) => tracing::info!(
                        "inner_event sync loop batch completed: target={:?}, lane={}, batch_size={}",
                        target,
                        lane,
                        batch_len
                    ),
                    Err(panic) => tracing::error!(
                        sync_target = ?target,
                        lane = lane,
                        batch_size = batch_len,
                        panic = ?panic,
                        "inner_event sync loop panicked while processing batch"
                    ),
                }
            }
            tracing::warn!("inner_event sync loop stopped: target={:?}", target);
        });
    }

    async fn sync_assets_once(
        ctx: &'static crate::context::Context,
        chain_code: String,
        token_address: AssetTokenKey,
        addr_list: Vec<String>,
        target: SyncTarget,
        retry_count: u32,
    ) -> Result<(), crate::error::service::ServiceError> {
        if addr_list.is_empty() {
            return Ok(());
        }

        match target {
            SyncTarget::Assets => {
                let chain_code_for_log = chain_code.clone();
                let token_address_for_log = token_address.clone();
                let addr_count = addr_list.len();
                let addr_list_for_call = addr_list.clone();
                tracing::info!(
                    "开始同步普通钱包资产: chain_code={}, token_address={}, addr_count={}, retry_count={}, addr_list={:?}",
                    chain_code,
                    token_address,
                    addr_count,
                    retry_count,
                    addr_list
                );
                let result = AssetsDomain::sync_assets_by_addr_chain_token(
                    ctx,
                    addr_list_for_call,
                    Some(chain_code),
                    token_address,
                )
                .await;

                match &result {
                    Ok(_) => tracing::info!(
                        "完成同步普通钱包资产: chain_code={}, token_address={}, addr_count={}, retry_count={}",
                        chain_code_for_log,
                        token_address_for_log,
                        addr_count,
                        retry_count
                    ),
                    Err(e) => tracing::error!(
                        "同步普通钱包资产失败: chain_code={}, token_address={}, addr_count={}, retry_count={}, error={}",
                        chain_code_for_log,
                        token_address_for_log,
                        addr_count,
                        retry_count,
                        e
                    ),
                }

                result
            }
            SyncTarget::ApiAssets => {
                let chain_code_for_log = chain_code.clone();
                let token_address_for_log = token_address.clone();
                let addr_count = addr_list.len();
                let addr_list_for_call = addr_list.clone();
                tracing::info!(
                    "开始同步 API 资产: chain_code={}, token_address={}, addr_count={}, retry_count={}, addr_list={:?}",
                    chain_code,
                    token_address,
                    addr_count,
                    retry_count,
                    addr_list
                );

                let result = ApiAssetsDomain::sync_assets_by_addr_chain_with_retry_with_ctx(
                    ctx,
                    addr_list_for_call,
                    Some(chain_code),
                    token_address,
                    retry_count,
                )
                .await;

                match &result {
                    Ok(_) => tracing::info!(
                        "完成同步 API 资产: chain_code={}, token_address={}, addr_count={}, retry_count={}",
                        chain_code_for_log,
                        token_address_for_log,
                        addr_count,
                        retry_count
                    ),
                    Err(e) => tracing::error!(
                        "同步 API 资产失败: chain_code={}, token_address={}, addr_count={}, retry_count={}, error={}",
                        chain_code_for_log,
                        token_address_for_log,
                        addr_count,
                        retry_count,
                        e
                    ),
                }

                result
            }
        }
    }
}

#[derive(Debug, Clone)]
enum SyncTarget {
    Assets,
    ApiAssets,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, timeout};
    use tokio_stream::StreamExt;

    #[tokio::test(flavor = "current_thread")]
    async fn api_event_buffer_deduplicates_addresses_before_drain() {
        let buffer = EventBuffer::new("test_dedup");
        let stream = buffer.wait_and_drain_stream(1);

        buffer.push_assets(SyncAssetsData::new_with_token_key(
            vec!["addr_1".to_string(), "addr_1".to_string(), "addr_2".to_string()],
            "tron".to_string(),
            AssetTokenKey::from_raw(Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf")),
        ));

        let mut stream = stream.await;
        let batch = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("wait for first drain")
            .expect("first batch exists");

        let mut addrs = batch.into_iter().map(|key| key.address).collect::<Vec<_>>();
        addrs.sort();

        assert_eq!(addrs, vec!["addr_1".to_string(), "addr_2".to_string()]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn api_event_buffer_ignores_empty_address_lists() {
        let buffer = EventBuffer::new("test_empty");
        buffer.push_assets(SyncAssetsData::new_with_token_key(
            Vec::new(),
            "tron".to_string(),
            AssetTokenKey::from_raw(Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf")),
        ));

        let result = timeout(Duration::from_millis(150), buffer.wait_and_drain_stream(1)).await;

        assert!(result.is_err(), "empty enqueue should not wake the drain loop");
    }

    #[test]
    fn api_event_priority_routes_to_separate_lanes() {
        let normal_buffer = EventBuffer::new("normal_test");
        let api_high_buffer = EventBuffer::new("api_high_test");
        let api_low_buffer = EventBuffer::new("api_low_test");

        let high_event = InnerEvent::ApiWalletSyncAssets(
            SyncAssetsData::new_with_token_key(
                vec!["addr_high".to_string()],
                "tron".to_string(),
                AssetTokenKey::from_raw(Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf")),
            )
            .with_priority(SyncPriority::High),
        );
        let low_event = InnerEvent::ApiWalletSyncAssets(
            SyncAssetsData::new_with_token_key(
                vec!["addr_low".to_string()],
                "tron".to_string(),
                AssetTokenKey::from_raw(Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf")),
            )
            .with_priority(SyncPriority::Low),
        );

        InnerEventHandle::dispatch_event(
            high_event,
            &normal_buffer,
            &api_high_buffer,
            &api_low_buffer,
        );
        InnerEventHandle::dispatch_event(
            low_event,
            &normal_buffer,
            &api_high_buffer,
            &api_low_buffer,
        );

        let high_len = api_high_buffer.buffer.lock().unwrap().len();
        let low_len = api_low_buffer.buffer.lock().unwrap().len();
        let normal_len = normal_buffer.buffer.lock().unwrap().len();

        assert_eq!(high_len, 1);
        assert_eq!(low_len, 1);
        assert_eq!(normal_len, 0);
    }
}
