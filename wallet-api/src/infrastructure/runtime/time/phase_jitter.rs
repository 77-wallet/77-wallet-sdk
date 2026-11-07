use rand::{Rng, SeedableRng};
use std::{
    process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Phase Jitter 工具
/// 用于确保多实例部署时不会同频 tick，避免资源竞争

/// 生成一个安全的随机种子
/// 结合 process_id、当前时间和 actor 名称，确保多实例种子不同
fn generate_seed(actor_name: &str) -> u64 {
    let pid = process::id() as u64;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
    let actor_hash = actor_name
        .as_bytes()
        .iter()
        .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));

    // 混合所有因素生成种子
    pid.wrapping_mul(31).wrapping_add(timestamp).wrapping_mul(31).wrapping_add(actor_hash)
}

/// 生成 phase offset（有界的随机延迟）
/// - duration: 基础间隔
/// - actor_name: actor 名称，用于生成种子
/// - 返回: 0 到 duration/2 之间的随机延迟
pub fn generate_phase_offset(duration: Duration, actor_name: &str) -> Duration {
    if duration.as_nanos() < 2 {
        return Duration::ZERO;
    }

    let seed = generate_seed(actor_name);
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    // 限制偏移为基础间隔的一半，避免扫描窗口过宽
    let max_offset = duration.as_nanos() / 2;
    let offset_nanos = rng.gen_range(0..max_offset);

    Duration::from_nanos(offset_nanos.min(u64::MAX as u128) as u64)
}

/// 执行 phase offset 延迟
/// - offset: 要延迟的时间
/// - shutdown_rx: shutdown 信号接收器
/// - 返回: true 表示延迟完成，false 表示被 shutdown 信号中断
pub async fn execute_phase_offset(
    offset: Duration,
    shutdown_rx: &mut tokio::sync::broadcast::Receiver<()>,
) -> bool {
    if offset.is_zero() {
        return true;
    }

    use crate::infrastructure::runtime::time::cancellable_sleep::cancellable_sleep;
    cancellable_sleep(offset, shutdown_rx).await
}
