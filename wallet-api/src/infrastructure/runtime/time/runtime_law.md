# Actor Runtime Laws

## 核心规则

### 1. Actor/Application 层禁止直接使用 tokio::time

**❌ DO NOT (Actor/Application 层):**
```rust
// 直接使用 tokio::time 方法
tokio::time::interval(Duration::from_secs(10));
tokio::time::sleep(Duration::from_millis(500)).await;
tokio::time::timeout(Duration::from_secs(5), future).await;
```

**✅ DO (Actor/Application 层):**
```rust
// 使用 runtime::time 模块
use wallet_api::infrastructure::runtime::time::*;

// 使用 Production Interval
let mut interval = new_production_interval(Duration::from_secs(10));
interval.tick().await;

// 使用 Cancellable Sleep
cancellable_sleep(Duration::from_millis(500), &mut shutdown_rx).await;

// 使用 Deadline
with_timeout(Duration::from_secs(5), future).await;
```

**✅ DO (Runtime Kernel 层):**
```rust
// 允许封装使用 tokio::time
let mut interval = tokio::time::interval(duration);
interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
```

**真实事故案例：**
- **2025-11**：直接使用 tokio::interval 导致移动端 replay storm，CPU spike 至 100%
- **2025-12**：未使用 CancellableSleep 导致 shutdown 超时，app 被系统杀死
- **2026-01**：未使用 PhaseJitter 导致多实例同时扫描，DB 连接池耗尽

---

### 2. 所有 interval 必须使用 ProductionInterval

**❌ DO NOT:**
```rust
// 直接使用 tokio::interval
let mut interval = tokio::time::interval(Duration::from_secs(10));
interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
interval.tick().await;
```

**✅ DO:**
```rust
// 使用 ProductionInterval
let mut interval = new_production_interval(Duration::from_secs(10));
interval.tick().await;
```

**原因：**
- ProductionInterval 自动处理启动屏障，避免第一次 tick 立即执行
- ProductionInterval 确保设置 MissedTickBehavior::Skip，避免 replay storm
- ProductionInterval 禁止克隆，防止多个 actor 共用一个时间流

---

### 3. 所有 sleep 必须使用 CancellableSleep

**❌ DO NOT:**
```rust
// 直接使用 tokio::sleep
 tokio::time::sleep(Duration::from_millis(5000)).await;
```

**✅ DO:**
```rust
// 使用 CancellableSleep
let slept = cancellable_sleep(Duration::from_millis(5000), &mut shutdown_rx).await;
if !slept {
    // 被 shutdown 信号中断，应该退出
    return;
}
```

**原因：**
- CancellableSleep 可被 shutdown 信号中断，避免 shutdown 超时
- CancellableSleep 优先处理 shutdown 信号，确保 app 能够及时退出
- CancellableSleep 防止移动端 app 被杀前 shutdown 超时

---

### 4. 所有定时扫描必须使用 PhaseJitter

**❌ DO NOT:**
```rust
// 直接启动扫描，不使用 jitter
async fn run(mut self) {
    let mut interval = new_production_interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        self.scan_round().await;
    }
}
```

**✅ DO:**
```rust
// 使用 PhaseJitter
async fn run(mut self) {
    // 生成 phase offset，避免多实例同时扫描
    let offset = generate_phase_offset(Duration::from_secs(60), "ScannerActor");
    if !execute_phase_offset(offset, &mut self.shutdown_rx).await {
        return;
    }
    
    let mut interval = new_production_interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = self.shutdown_rx.recv() => break,
            _ = interval.tick() => {
                self.scan_round().await;
            },
        }
    }
}
```

**原因：**
- PhaseJitter 确保多实例部署时不会同频 tick
- PhaseJitter 避免多实例同时扫描，导致 RPC 和 DB 同时爆发
- PhaseJitter 使用安全的 seed，确保多实例种子不同

---

### 5. 所有 spawn 必须尊重 SPAWN_GUARD

**❌ DO NOT:**
```rust
// 直接 spawn，不考虑系统资源
 tokio::spawn(async move {
    // 执行任务
 });
```

**✅ DO:**
```rust
// 使用 SPAWN_GUARD 限制并发数量
 if let Ok(permit) = SPAWN_GUARD.try_acquire() {
    tokio::spawn(async move {
        let _permit = permit;
        // 执行任务
    });
} else {
    // spawn 限制命中，记录警告
    warn!("Spawn limit reached, task skipped");
}
```

**原因：**
- SPAWN_GUARD 确保系统资源不被耗尽
- SPAWN_GUARD 防止 spawn storm，避免系统崩溃
- SPAWN_GUARD 提供可观测性，便于监控系统状态

---

## 最佳实践

### 1. 使用统一的时间单位

**✅ DO:**
```rust
// 使用 Duration 常量，确保时间单位统一
const SCAN_INTERVAL: Duration = Duration::from_secs(60);
const JITTER_MAX: Duration = Duration::from_millis(5000);
```

**原因：**
- 统一时间单位，避免时间计算错误
- 提高代码可读性，便于维护

### 2. 合理设置时间参数

**✅ DO:**
```rust
// 根据实际需求设置合理的时间参数
// 扫描间隔：60秒，平衡实时性和系统负载
let mut interval = new_production_interval(Duration::from_secs(60));

// Jitter：0-30秒，避免多实例同时扫描
let offset = generate_phase_offset(Duration::from_secs(60), "ScannerActor");
```

**原因：**
- 合理的时间参数平衡实时性和系统负载
- 避免过于频繁的扫描导致系统压力过大

### 3. 监控时间相关指标

**✅ DO:**
```rust
// 监控 loop latency，作为移动端 thermal 报警的早期指标
let mut metrics = LoopLatencyMetrics::new();
loop {
    metrics.record_loop_start();
    
    // 执行扫描逻辑
    self.scan_round().await;
    
    metrics.record_loop_end();
    
    // 定期上报指标
    if need_report() {
        report_metrics(metrics.average_loop_latency_ms());
    }
}
```

**原因：**
- 监控时间相关指标，便于发现性能问题
- 早期发现移动端 thermal 问题，避免系统崩溃

---

## 总结

遵循这些 Runtime Laws 可以确保：

1. **系统稳定性**：避免 replay storm、shutdown 超时等问题
2. **移动端友好**：减少电量消耗，避免 app 被系统杀死
3. **多实例安全**：避免多实例同时扫描，导致资源竞争
4. **可观测性**：提供完整的 metrics，便于监控系统状态
5. **代码一致性**：统一时间工具使用，提高代码可维护性

违反这些规则可能导致：
- **性能问题**：CPU spike、内存泄漏
- **稳定性问题**：app 崩溃、系统死锁
- **运维问题**：难以监控、难以调试

请严格遵守这些 Runtime Laws，确保系统的长期稳定运行。