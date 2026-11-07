pub mod api_wallet;
pub mod wallet;

use crate::response::response::BackendResponse;
use dashmap::DashMap;
use fastrand;
use once_cell::sync::Lazy;
use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::Semaphore;
use url::Url;
use wallet_transport::errors::TransportError;

/// HostClass 用于分类主机，避免 semaphore map 无限增长
#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum HostClass {
    /// 后端 API 主机
    Backend,
}

/// HTTP 任务计数器
static HTTP_PENDING_TASKS: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

/// PendingGuard 用于确保 HTTP 任务计数准确
/// 使用 RAII 模式，确保无论任务成功还是失败，计数都会正确更新
struct HttpPendingGuard;

impl HttpPendingGuard {
    /// 创建一个新的 PendingGuard
    /// 创建时会增加计数器
    pub fn new() -> Self {
        HTTP_PENDING_TASKS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for HttpPendingGuard {
    /// 当 PendingGuard 被 drop 时，会减少计数器
    fn drop(&mut self) {
        HTTP_PENDING_TASKS.fetch_sub(1, Ordering::Relaxed);
    }
}

static HOST_LIMITERS: Lazy<DashMap<(HostClass, String), (Arc<Semaphore>, std::time::Instant)>> =
    Lazy::new(DashMap::new);
static GLOBAL_FALLBACK_LIMITER: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(8)));
static COOLED_HOSTS: Lazy<DashMap<String, std::time::Instant>> = Lazy::new(DashMap::new);

/// 用于保证清理任务只初始化一次
static CLEANUP_INIT: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

/// 初始化后台清理任务
fn initialize_cleanup_task() {
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(_) => {
            tracing::warn!("Skip cleanup task init: no Tokio runtime in current thread");
            return;
        }
    };

    if CLEANUP_INIT.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }

    handle.spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await; // 每 10 分钟清理一次
            cleanup_limiters();
        }
    });
    tracing::info!("Background cleanup task initialized");
}

/// 清理过期的 limiters（TTL 24小时）
fn cleanup_limiters() {
    let now = std::time::Instant::now();
    let before_cleanup = HOST_LIMITERS.len();
    HOST_LIMITERS.retain(|_, (_, last_used)| {
        now.duration_since(*last_used) < std::time::Duration::from_secs(86400)
    });
    let after_cleanup = HOST_LIMITERS.len();
    if before_cleanup != after_cleanup {
        tracing::info!("Cleaned up {} expired limiters", before_cleanup - after_cleanup);
    }
}

fn canonical_host(host: &str) -> String {
    let trimmed_host = host.trim().trim_end_matches('/');

    // 处理空host，避免limiter污染
    if trimmed_host.is_empty() {
        tracing::error!("empty host passed into limiter");
        return "invalid://host".into();
    }

    let mut input = trimmed_host.to_lowercase();

    // 处理无协议的主机名
    if !input.starts_with("http://") && !input.starts_with("https://") {
        input = format!("https://{}", input);
    }

    match Url::parse(&input) {
        Ok(url) => {
            let scheme = url.scheme();
            let host_str = match url.host() {
                Some(host) => match host {
                    url::Host::Ipv6(addr) => format!("[{}]", addr),
                    _ => host.to_string(),
                },
                None => return trimmed_host.to_string(),
            };
            let port = url.port();

            // 构建canonical host，包含scheme，处理默认端口
            match (scheme, port) {
                ("http", Some(80)) | ("https", Some(443)) => {
                    format!("{}://{}", scheme, host_str)
                }
                (_, Some(p)) => {
                    format!("{}://{}:{}", scheme, host_str, p)
                }
                _ => {
                    format!("{}://{}", scheme, host_str)
                }
            }
        }
        Err(_) => {
            // 解析失败时fallback到原始主机名
            trimmed_host.to_string()
        }
    }
}

fn host_limiter(host: &str, class: HostClass) -> Arc<Semaphore> {
    tracing::debug!("Starting host_limiter for host {}, class: {:?}", host, class);

    let canonical = canonical_host(host);

    tracing::debug!("Canonical host: {}", canonical);

    // 处理无效主机，使用全局 fallback limiter
    if canonical == "invalid://host" {
        tracing::info!("Invalid host detected, using fallback limiter");
        return GLOBAL_FALLBACK_LIMITER.clone();
    }

    // 检查主机是否在冷却期内
    if let Some(cooled_entry) = COOLED_HOSTS.get(&canonical) {
        let cooled_until = cooled_entry.value();
        if std::time::Instant::now() < *cooled_until {
            tracing::info!(
                "Host {} is in cooldown until {:?}, using fallback limiter",
                canonical,
                cooled_until
            );
            return GLOBAL_FALLBACK_LIMITER.clone();
        } else {
            // 冷却期已过，移除冷却标记
            tracing::debug!("Host {} cooldown expired, removing from cooled hosts", canonical);
            COOLED_HOSTS.remove(&canonical);
        }
    }

    let key = (class.clone(), canonical.clone());
    tracing::debug!("Using key: {:?}", key);

    // 第一阶段：尝试获取现有 limiter（无锁风险）
    if let Some(mut e) = HOST_LIMITERS.get_mut(&key) {
        tracing::debug!("Host {} already has a limiter, using existing one", canonical);
        // 更新最后使用时间
        e.1 = std::time::Instant::now();
        tracing::debug!("Using existing limiter for key: {:?}", key);
        return e.0.clone();
    }

    // 第二阶段：创建新 limiter
    tracing::debug!("Host {} does not have a limiter, creating new one", canonical);

    // 创建新的 semaphore
    let limit = match class {
        HostClass::Backend => 8,
    };
    tracing::debug!("Creating new limiter with limit: {} for class: {:?}", limit, class);

    // 使用 or_insert_with 避免 entry 内的锁操作
    let sem = HOST_LIMITERS
        .entry(key)
        .or_insert_with(|| (Arc::new(Semaphore::new(limit)), std::time::Instant::now()))
        .0
        .clone();

    sem
}

#[derive(Debug, Clone)]
pub struct BackendApi {
    pub base_url: String,
    pub client: wallet_transport::client::HttpClient,
    pub(crate) aes_cbc_cryptor: wallet_utils::cbc::AesCbcCryptor,
}

impl BackendApi {
    pub fn new(
        backend_url: Option<String>,
        headers_opt: Option<HashMap<String, String>>,
        aes_cbc_cryptor: wallet_utils::cbc::AesCbcCryptor,
    ) -> Result<Self, crate::Error> {
        // 初始化后台清理任务
        initialize_cleanup_task();

        let url = backend_url.unwrap_or(crate::consts::BASE_URL.to_string());

        let mut headers_opt = headers_opt.unwrap_or_default();
        headers_opt.insert("Accept-Encoding".to_string(), "identity".to_string());

        let timeout = Some(std::time::Duration::from_secs(15));
        Ok(Self {
            base_url: url.to_string(),
            client: build_http_client(&url, Some(headers_opt), timeout)?,
            aes_cbc_cryptor,
        })
    }

    async fn send_with_limit<R, F, Fut>(
        &self,
        host: &str,
        class: HostClass,
        f: F,
    ) -> Result<R, crate::Error>
    where
        R: crate::response::BackendRespExt + Debug + Send + 'static,
        F: Fn() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<R, crate::Error>> + Send + 'static,
    {
        tracing::debug!("send_with_limit {:?}", host);
        let start = std::time::Instant::now();
        let res = self.send_with_limit_inner(host, class, f).await;
        let duration = start.elapsed();
        tracing::debug!("send_with_limit {:?}, duration: {:?}", res, duration);
        res
    }

    async fn send_with_limit_inner<R, F, Fut>(
        &self,
        host: &str,
        class: HostClass,
        f: F,
    ) -> Result<R, crate::Error>
    where
        R: crate::response::BackendRespExt + Debug,
        F: Fn() -> Fut, // ✅ Fn 而不是 FnOnce
        Fut: std::future::Future<Output = Result<R, crate::Error>>,
    {
        // 克隆 class 以便在多处使用
        let class_clone = class.clone();
        let h = host_limiter(host, class_clone);

        // 根据 HostClass 设置 acquire timeout
        let acquire_timeout = match class {
            HostClass::Backend => std::time::Duration::from_secs(3),
        };

        tracing::debug!(
            "Starting send_with_limit_inner for host {}, class: {:?}, acquire_timeout: {:?}",
            host,
            class,
            acquire_timeout
        );

        // 设置重试参数
        let max_attempts = match class {
            HostClass::Backend => 3,
        };

        // 设置总请求时间预算（deadline）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

        tracing::debug!("Request deadline set to {:?}, max_attempts: {}", deadline, max_attempts);

        let mut backoff = 1000u64;
        let mut consecutive_timeouts = 0;
        let canonical_host = canonical_host(host);

        for attempt in 1..=max_attempts {
            // 检查是否超过总请求时间预算
            if std::time::Instant::now() > deadline {
                tracing::warn!("Request deadline exceeded for host {}, attempt: {}", host, attempt);
                break;
            }

            tracing::debug!(
                "Starting HTTP attempt {} for host {}, canonical: {}",
                attempt,
                host,
                canonical_host
            );

            let req_start = std::time::Instant::now();
            let attempt_result = {
                // 每次 attempt 只在真正发请求时持有 permit，避免在 backoff sleep 期间占用并发槽位。
                let acquire_start = std::time::Instant::now();
                let _permit = tokio::time::timeout(acquire_timeout, h.clone().acquire_owned())
                    .await
                    .map_err(|_| {
                        tracing::warn!(
                            "semaphore acquire timeout for host {}, class: {:?}, timeout: {:?}",
                            host,
                            class,
                            acquire_timeout
                        );
                        crate::Error::Timeout
                    })??;
                let acquire_duration = acquire_start.elapsed();
                if acquire_duration > std::time::Duration::from_millis(50) {
                    tracing::info!(
                        "Acquired semaphore for host {}, class: {:?}, duration: {:?}",
                        host,
                        class,
                        acquire_duration
                    );
                } else {
                    tracing::debug!(
                        "Acquired semaphore for host {}, class: {:?}, duration: {:?}",
                        host,
                        class,
                        acquire_duration
                    );
                }

                // 创建 HttpPendingGuard，确保任务计数准确
                let _guard = HttpPendingGuard::new();

                // 计算剩余时间，确保不超过 deadline
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                let request_timeout = remaining.min(std::time::Duration::from_secs(20));

                tracing::debug!(
                    "Request timeout set to {:?} (remaining: {:?})
",
                    request_timeout,
                    remaining
                );

                tokio::time::timeout(request_timeout, f()).await
            };

            match attempt_result {
                Ok(Ok(res)) => {
                    let req_duration = req_start.elapsed();
                    tracing::debug!(
                        "HTTP attempt {} success, duration: {:?}",
                        attempt,
                        req_duration
                    );

                    if !res.success() && res.is_rate_limited() {
                        tracing::info!("Rate limited response, retrying...");
                    } else {
                        tracing::info!("Request completed successfully for host {}", host);
                        return Ok(res);
                    }
                }
                Ok(Err(e)) => {
                    let req_duration = req_start.elapsed();
                    tracing::debug!(
                        "HTTP attempt {} failed, duration: {:?}, error: {:?}",
                        attempt,
                        req_duration,
                        e
                    );

                    if e.is_rate_limited() {
                        tracing::info!("Rate limited error, retrying...");
                    } else {
                        tracing::info!("Request failed with non-retryable error: {:?}", e);
                        return Err(e);
                    }
                }
                Err(_) => {
                    tracing::debug!("HTTP timeout attempt {}", attempt);
                    consecutive_timeouts += 1;

                    // 当连续 timeout >= 3 时，冷却主机 30 秒
                    if consecutive_timeouts >= 3 {
                        let cooled_until =
                            std::time::Instant::now() + std::time::Duration::from_secs(30);
                        COOLED_HOSTS.insert(canonical_host.clone(), cooled_until);
                        tracing::warn!(
                            "Host {} cooled down until {:?} due to {} consecutive timeouts",
                            canonical_host,
                            cooled_until,
                            consecutive_timeouts
                        );
                    }
                }
            }

            // 检查是否还有时间重试
            if std::time::Instant::now() > deadline {
                tracing::warn!("Request deadline exceeded for host {}, attempt: {}", host, attempt);
                break;
            }

            // ⭐ jitter 防同步风暴
            let jitter = fastrand::u64(..200);
            let sleep_ms = backoff + jitter;

            tracing::debug!("Backing off {}ms before next attempt", sleep_ms);

            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;

            backoff = (backoff * 2).min(10_000);
        }

        tracing::warn!("Max retry reached host {}", host);
        Err(crate::Error::RateLimited)
    }

    pub fn replace_base_url(&mut self, base_url: &str) {
        self.base_url = base_url.to_string();
        self.client.replace_base_url(base_url);
    }

    pub async fn post_request<T, R>(&self, endpoint: &str, req: T) -> Result<R, crate::Error>
    where
        T: serde::Serialize + std::fmt::Debug,
        R: serde::de::DeserializeOwned + serde::Serialize + Debug,
    {
        let res = self.client.post(endpoint).json(req).send::<BackendResponse>().await?;
        res.process::<R>(&self.aes_cbc_cryptor)
    }

    // 发送一个字符串的请求.
    pub async fn post_req_string<T>(&self, endpoint: &str, body: String) -> Result<T, crate::Error>
    where
        T: serde::de::DeserializeOwned + serde::Serialize + Debug,
    {
        let res = self.client.post(endpoint).body(body).send::<BackendResponse>().await?;
        res.process::<T>(&self.aes_cbc_cryptor)
    }

    pub async fn post_default(
        &self,
        endpoint: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, crate::Error> {
        let host = self.base_url.clone();

        // 👇 关键：提前 clone 成 owned
        let endpoint = endpoint.to_string();
        let body = body.to_string();
        let client = self.client.clone();
        let cryptor = self.aes_cbc_cryptor.clone();

        let res: BackendResponse = self
        .send_with_limit(&host, HostClass::Backend, move || {
            let endpoint = endpoint.clone();
            let body = body.clone();
            let client = client.clone();

            async move {
                Ok(client
                    .post(&endpoint)
                    .body(body)
                    .send::<BackendResponse>()
                    .await?)
            }
        })
        .await?;
        res.process::<serde_json::Value>(&cryptor)
    }

    pub(crate) async fn post_backend_json<R>(
        &self,
        endpoint: &str,
        req: serde_json::Value,
    ) -> Result<R, crate::Error>
    where
        R: serde::de::DeserializeOwned + serde::Serialize + Debug,
    {
        let host = self.base_url.clone();
        let endpoint = endpoint.to_string();
        let client = self.client.clone();
        let req = req.clone();
        let cryptor = self.aes_cbc_cryptor.clone();

        let res: BackendResponse = self
            .send_with_limit(&host, HostClass::Backend, move || {
                let endpoint = endpoint.clone();
                let client = client.clone();
                let req = req.clone();

                async move { Ok(client.post(&endpoint).json(req).send::<BackendResponse>().await?) }
            })
            .await?;
        res.process::<R>(&cryptor)
    }

    pub(crate) async fn post_backend_empty<R>(&self, endpoint: &str) -> Result<R, crate::Error>
    where
        R: serde::de::DeserializeOwned + serde::Serialize + Debug,
    {
        let host = self.base_url.clone();
        let endpoint = endpoint.to_string();
        let client = self.client.clone();
        let cryptor = self.aes_cbc_cryptor.clone();

        let res: BackendResponse = self
            .send_with_limit(&host, HostClass::Backend, move || {
                let endpoint = endpoint.clone();
                let client = client.clone();

                async move { Ok(client.post(&endpoint).send::<BackendResponse>().await?) }
            })
            .await?;
        res.process::<R>(&cryptor)
    }

    pub(crate) async fn get_backend<R>(&self, endpoint: &str) -> Result<R, crate::Error>
    where
        R: serde::de::DeserializeOwned + serde::Serialize + Debug,
    {
        let host = self.base_url.clone();
        let endpoint = endpoint.to_string();
        let client = self.client.clone();
        let cryptor = self.aes_cbc_cryptor.clone();

        let res: BackendResponse = self
            .send_with_limit(&host, HostClass::Backend, move || {
                let endpoint = endpoint.clone();
                let client = client.clone();

                async move { Ok(client.get(&endpoint).send::<BackendResponse>().await?) }
            })
            .await?;
        res.process::<R>(&cryptor)
    }

    // 发送一个字符串的请求.
    pub async fn post_req_str<T>(
        &self,
        endpoint: &str,
        body: &serde_json::Value,
    ) -> Result<T, crate::Error>
    where
        T: serde::de::DeserializeOwned + serde::Serialize + Debug,
    {
        let host = self.base_url.clone();

        let endpoint = endpoint.to_string();
        let body = body.to_string();
        let client = self.client.clone();
        let cryptor = self.aes_cbc_cryptor.clone();

        let res: BackendResponse = self
        .send_with_limit(&host, HostClass::Backend, move || {
            let endpoint = endpoint.clone();
            let body = body.clone();
            let client = client.clone();

            async move {
                Ok(client
                    .post(&endpoint)
                    .body(body)
                    .send::<BackendResponse>()
                    .await?)
            }
        })
        .await?;
        res.process::<T>(&cryptor)
    }

    pub async fn post_api_backend<T, R>(
        &self,
        endpoint: &str,
        req: T,
    ) -> Result<Option<R>, crate::Error>
    where
        T: serde::Serialize + Debug + Clone + Send + 'static,
        R: serde::de::DeserializeOwned + serde::Serialize,
    {
        let host = self.base_url.clone();

        let endpoint = endpoint.to_string();
        let client = self.client.clone();
        let req = req.clone();

        let res: crate::response::api_response::ApiBackendResponse = self
            .send_with_limit(&host, HostClass::Backend, move || {
                let endpoint = endpoint.clone();
                let client = client.clone();
                let req = req.clone();
                tracing::info!("endpoint: {}", endpoint);
                async move { Ok(client.post(&endpoint).json(&req).send().await?) }
            })
            .await?;
        tracing::debug!("post_api_backend {:?}", res);
        res.process::<R>()
    }
}

fn build_http_client(
    base_url: &str,
    headers_opt: Option<HashMap<String, String>>,
    timeout: Option<std::time::Duration>,
) -> Result<wallet_transport::client::HttpClient, crate::Error> {
    let mut headers = HeaderMap::new();

    headers.append(header::ACCEPT, HeaderValue::from_static("application/json"));
    headers.append(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));

    if let Some(opt) = headers_opt {
        for (key, value) in opt {
            let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|e| {
                crate::Error::Backend(Some(format!("invalid header name `{key}`: {e}")))
            })?;
            let header_value = HeaderValue::from_str(&value).map_err(|e| {
                crate::Error::Backend(Some(format!("invalid header value for `{key}`: {e}")))
            })?;
            headers.append(header_name, header_value);
        }
    }

    // Some sandboxed environments (and some CI setups on macOS) cannot access SystemConfiguration,
    // which can make the system proxy resolver panic. Allow opting out via env var.
    let mut builder = reqwest::ClientBuilder::new().default_headers(headers);
    if std::env::var_os("WALLET_TRANSPORT_NO_PROXY").is_some() {
        builder = builder.no_proxy();
    }

    #[cfg(feature = "accept_invalid_certs")]
    {
        builder = builder.danger_accept_invalid_certs(true);
    }

    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }

    let client = builder
        .build()
        .map_err(|e| TransportError::Utils(wallet_utils::error::Error::Http(e.into())))?;

    Ok(wallet_transport::client::HttpClient { base_url: base_url.to_owned(), client })
}

#[cfg(test)]
mod tests {
    use super::{BackendApi, build_http_client, canonical_host};
    use std::collections::HashMap;

    #[test]
    fn canonical_host_normalizes_scheme_case_and_default_port() {
        assert_eq!(canonical_host("https://EXAMPLE.com:443/"), "https://example.com");
        assert_eq!(canonical_host("example.com"), "https://example.com");
    }

    #[test]
    fn build_http_client_returns_error_for_invalid_header_name() {
        let mut headers = HashMap::new();
        headers.insert("bad\nheader".to_string(), "value".to_string());
        let err = build_http_client("https://example.com", Some(headers), None)
            .expect_err("invalid header name should return error");

        match err {
            crate::Error::Backend(Some(msg)) => assert!(msg.contains("invalid header name")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn backend_api_new_is_safe_without_runtime() {
        let mut headers = HashMap::new();
        headers.insert("clientId".to_string(), "offline-client".to_string());
        headers.insert("AW-SEC-ID".to_string(), "offline-aw".to_string());

        let cryptor = wallet_utils::cbc::AesCbcCryptor::new("1234567890abcdef", "abcdef1234567890");
        let api = BackendApi::new(Some("https://example.com".to_string()), Some(headers), cryptor);
        assert!(api.is_ok());
    }
}
