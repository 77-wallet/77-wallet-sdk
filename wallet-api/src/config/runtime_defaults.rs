use std::time::Duration;

// 收口本轮线上稳定性修复涉及的运行时默认值，避免阈值散落在多个模块中。
// 这里仅放“默认值”，不放业务流程代码，调用侧按模块取一组配置即可。

#[derive(Debug, Clone, Copy)]
pub struct ApiAssetsDefaults {
    // 小钱包沿用 v2 聚合路径，大钱包走 v3 优先策略。
    pub small_wallet_address_threshold: i64,
    // 大钱包首算给更宽的执行预算，避免首笔请求过早超时。
    pub large_wallet_v3_timeout: Duration,
    // 同 key 成功结果的短 TTL，用于吸收页面停留期间的瞬时重复请求。
    pub total_cache_ttl: Duration,
    // 大钱包默认禁止回退 v2，防止回退到重 SQL 路径放大 DB 压力。
    pub allow_v2_fallback_large_wallet: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TaskQueueDefaults {
    pub historical_task_offset: u8,
    pub dispatch_max_concurrent: usize,
    pub initialization_limit: usize,
    pub backend_api_limit: usize,
    pub mqtt_limit: usize,
    pub common_limit: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RecoveryDefaults {
    // 后台任务池并发上限，受 SQLite 连接池容量约束。
    pub background_task_pool_max_concurrent: usize,
    // 恢复 worker 每轮认领量上限，降低单轮突发压力。
    pub asset_query_max_claims_per_round: usize,
    // 账变触发的持久化余额同步每轮认领量上限。
    pub api_asset_sync_max_claims_per_round: usize,
    // 已完成的 API 资产同步状态保留时长，避免 dirty-state 表无限增长。
    pub api_asset_sync_done_retention_secs: u64,
    // 已完成的 API 资产同步状态最大保留条数，作为时间保留之外的硬上限。
    pub api_asset_sync_done_max_rows: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct UnlockSessionDefaults {
    // 解锁会话的轮换周期，用于定期刷新内存中的 session material。
    pub rotation_interval_secs: u64,
    // 轮换检查的轮询周期，决定多久检查一次是否需要刷新会话。
    pub rotation_check_interval_secs: u64,
}

pub const fn api_assets() -> ApiAssetsDefaults {
    ApiAssetsDefaults {
        small_wallet_address_threshold: 200,
        large_wallet_v3_timeout: Duration::from_secs(30),
        total_cache_ttl: Duration::from_millis(3000),
        allow_v2_fallback_large_wallet: false,
    }
}

pub const fn task_queue() -> TaskQueueDefaults {
    // 任务队列限额属于“容量保护阈值”，统一从这里下发到 scheduler/dispatcher。
    TaskQueueDefaults {
        historical_task_offset: 10,
        dispatch_max_concurrent: 16,
        initialization_limit: 3,
        backend_api_limit: 6,
        mqtt_limit: 12,
        common_limit: 2,
    }
}

pub const fn recovery() -> RecoveryDefaults {
    RecoveryDefaults {
        background_task_pool_max_concurrent: 6,
        asset_query_max_claims_per_round: 8,
        api_asset_sync_max_claims_per_round: 16,
        api_asset_sync_done_retention_secs: 24 * 60 * 60,
        api_asset_sync_done_max_rows: 50_000,
    }
}

pub const fn unlock_session() -> UnlockSessionDefaults {
    #[cfg(test)]
    {
        return UnlockSessionDefaults {
            rotation_interval_secs: 1,
            rotation_check_interval_secs: 1,
        };
    }

    #[cfg(not(test))]
    {
        return UnlockSessionDefaults {
            rotation_interval_secs: 300,
            rotation_check_interval_secs: 10,
        };
    }
}
