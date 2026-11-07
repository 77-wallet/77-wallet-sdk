#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    // 波场活跃权限超过 8个
    #[error("actives permission more than eight")]
    ActivesPermissionMore,
    // 所有权重的和小于 阈值
    #[error("weight less tran threshold")]
    WeightLessThreshold,
    // 活跃权限为空(至少保留一个活跃权限)
    #[error("miss actives permission")]
    MissActivesPermission,
    // 未找到对应的活跃权限
    #[error("actives permission not found")]
    ActivesPermissionNotFound,
    // 不支持的操作类型(delete update new)
    #[error("un support op type: = {0}")]
    UnSupportOpType(String),
    // 链不支持权限
    #[error("un support permission chain")]
    UnSupportPermissionChain,
}

impl PermissionError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            PermissionError::ActivesPermissionMore => 4300,
            PermissionError::WeightLessThreshold => 4301,
            PermissionError::MissActivesPermission => 4302,
            PermissionError::ActivesPermissionNotFound => 4303,
            PermissionError::UnSupportOpType(_) => 4304,
            PermissionError::UnSupportPermissionChain => 4305,
        }
    }
}
