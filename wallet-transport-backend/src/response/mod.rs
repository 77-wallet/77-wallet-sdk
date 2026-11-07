pub mod api_response;
pub(crate) mod response;

pub trait BackendRespExt {
    /// HTTP/业务层返回码（能映射出是否限流）
    fn code(&self) -> Option<i64>;

    /// 是否表示成功
    fn success(&self) -> bool;

    /// 错误消息（可选）
    fn message(&self) -> Option<&str>;

    /// 是否是“被限流”语义
    fn is_rate_limited(&self) -> bool {
        matches!(self.code(), Some(429 | 1015))
    }

    /// 是否值得重试（可扩展）
    fn is_retryable(&self) -> bool {
        self.is_rate_limited()
    }
}
