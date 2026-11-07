# 旧 Worker 到 Shadow Worker 函数映射表

## 核心设计原则

1. **Worker 不决策**：只执行 Scanner/Dispatcher 发来的命令
2. **纯执行模型**：无内存态，kill -9 安全
3. **DB 作为唯一数据源**：所有状态和数据都从 DB 读取
4. **无逻辑锁**：避免死锁和假忙
5. **明确的命令边界**：每个命令只做一件事

## 旧 Worker 到 Shadow Worker 函数映射

| 旧 Worker（process_collect_tx_send.rs） | Shadow Worker（collect_worker.rs） | 说明 |
|----------------------------------------|-----------------------------------|------|
| `process_collect_single_tx`            | `process_build_tx`                | 处理单个归集交易，从 INIT 状态构建并广播交易 |
| `handle_collect_tx_success`            | `handle_collect_tx_success`       | 处理交易成功，更新状态和 nonce |
| `handle_collect_tx_failed`             | `handle_collect_tx_failed`        | 处理交易失败，更新状态 |
| `check_digest`                         | `check_digest`                    | 验证交易摘要 |
| `resolve_collect_to_addr`              | `resolve_collect_to_addr`         | 解析归集执行地址 |
| `gen_transfer_req`                     | `gen_transfer_req`                | 生成转账请求 |
| `process_recovered_tx`（ApiTransDomain） | `recover_tx`                     | 处理已恢复的交易 |
| `get_eth_nonce`                        | `get_nonce`                       | 获取并更新 nonce（使用 `upsert_and_get_api_nonce`） |
| `check_fee`（CheckFee trait）          | `check_fee`                       | 检查手续费是否充足 |
| -                                      | `check_balance`                   | 检查余额是否充足（新增） |
| -                                      | `process_broadcast`               | 单独处理广播命令（新增，分离关注点） |
| -                                      | `process_confirm`                 | 处理确认命令（新增） |
| -                                      | `process_ack`                     | 处理上报 ACK 命令（新增） |

## 旧 Worker 功能在 Shadow Worker 中的实现方式

| 旧 Worker 功能 | Shadow Worker 实现方式 |
|---------------|------------------------|
| **批量处理** | 由 Scanner 定期扫描 DB，Dispatcher 分发命令 |
| **单交易处理** | 由 Scanner 扫描到单个交易，Dispatcher 分发命令 |
| **地址锁** | 使用 `AddressLockManager`，但仅在必要时使用 |
| **全局并发控制** | 使用 `Semaphore` 控制 RPC/链上执行的并发度 |
| **交易恢复** | 在 `process_build_tx` 开始时检查并处理已有 tx_hash 的交易 |
| **状态更新** | 只在交易成功或失败时更新状态，build 成功不直接推进状态 |
| **手续费检查** | 独立的 `check_fee` 函数，包含安全缓冲区 |
| **余额检查** | 独立的 `check_balance` 函数，验证转账金额和手续费 |
| **nonce 管理** | 使用 `upsert_and_get_api_nonce` 作为唯一 nonce 源 |

## Shadow Worker 核心命令处理流程

### 1. BuildTx 命令处理流程

```rust
async fn process_build_tx(&self, trade_no: String) -> Result<(), ServiceError> {
    // 1. 获取交易实体
    let mut req = self.get_collect_entity(&trade_no).await?;
    
    // 2. 状态校验：只处理 INIT 状态
    self.assert_state(&req, ApiCollectStatus::Init)?;
    
    // 3. 获取地址锁，保护地址级并发
    let _addr_guard = self.address_locks.acquire(&req.from_addr).await?;
    
    // 4. 获取全局信号量，控制 RPC/链上执行并发度
    let _global_guard = self.global_sem.acquire().await?;
    
    // 5. 交易恢复：如果已有 tx_hash，检查链上状态
    if let Some(tx_resp) = self.recover_tx(&req).await? {
        return self.handle_collect_tx_success(&req, tx_resp, req.nonce as u64).await;
    }
    
    // 6. 解析执行地址
    let exec_to_addr = self.resolve_collect_to_addr(&req).await?;
    
    // 7. 检查手续费
    self.check_fee(&req).await?;
    
    // 8. 检查交易摘要
    self.check_digest(&req).await?;
    
    // 9. 获取 nonce
    let nonce = self.get_nonce(&req.from_addr, &req.chain_code).await?;
    
    // 10. 生成转账请求
    let transfer_req = self.gen_transfer_req(&req, &exec_to_addr).await?;
    
    // 11. 构建并广播交易
    let (tx_hash, raw_tx, fee) = self.build_transfer_raw(&transfer_req).await?;
    
    // 12. 保存 tx_hash 和 raw_tx 到 DB
    self.save_tx_data(&req.trade_no, &tx_hash, &raw_tx, &fee).await?;
    
    // 13. 广播交易
    let tx_resp = self.broadcast_transfer(&req.chain_code, raw_tx).await?;
    
    // 14. 处理交易结果
    match tx_resp {
        Some(tx) => self.handle_collect_tx_success(&req, tx, nonce).await,
        None => Ok(()),
    }
}
```

### 2. Broadcast 命令处理流程

```rust
async fn process_broadcast(&self, trade_no: String) -> Result<(), ServiceError> {
    // 1. 获取交易实体
    let req = self.get_collect_entity(&trade_no).await?;
    
    // 2. 状态校验：只处理 BUILT 状态
    self.assert_state(&req, ApiCollectStatus::Built)?;
    
    // 3. 获取地址锁，保护地址级并发
    let _addr_guard = self.address_locks.acquire(&req.from_addr).await?;
    
    // 4. 获取全局信号量，控制 RPC/链上执行并发度
    let _global_guard = self.global_sem.acquire().await?;
    
    // 5. 检查交易数据完整性
    self.assert_tx_data(&req)?;
    
    // 6. 广播交易
    let raw_tx = self.deserialize_raw_tx(&req.raw_tx).await?;
    let tx_resp = self.broadcast_transfer(&req.chain_code, raw_tx).await?;
    
    // 7. 处理交易结果
    match tx_resp {
        Some(tx) => self.handle_collect_tx_success(&req, tx, req.nonce as u64).await,
        None => Ok(()),
    }
}
```

## Shadow Worker 不该做的事（断言保护）

1. **禁止处理终态交易**：
   ```rust
debug_assert!(!req.status.is_terminal(), "Terminal status should not be processed");
```

2. **禁止在 build 成功后直接推进状态**：
   ```rust
// 只在 broadcast 成功或失败时更新状态，build 成功不直接推进
```

3. **禁止维护内存态**：
   ```rust
// 无 processing_trade、batch_running 等内存态变量
```

4. **禁止 Worker 内部决策**：
   ```rust
// 只执行命令，不做调度决策
```

5. **禁止使用逻辑锁**：
   ```rust
// 只使用物理锁（地址锁、信号量），不使用逻辑锁
```

## 关键设计改进

1. **命令驱动**：每个操作都是明确的命令，职责单一
2. **状态驱动**：基于 DB 状态而非内存态
3. **无副作用**：每个命令的副作用都明确且可控
4. **易于测试**：每个命令都可以独立测试
5. **容错设计**：部分失败不影响整体系统
6. **清晰的错误处理**：每个错误都有明确的处理方式

## 旧 Worker 已被放弃的功能

* ❌ TradeGuard
* ❌ processing_trade
* ❌ batch_running
* ❌ Worker 内部 retry / 决策
* ❌ Worker 维护“当前正在做什么”的内存态
* ❌ build 成功就推进状态

这些功能正是旧系统出现“卡死 / 假忙 / 重启失效”的根源，Shadow Worker 从根本上解决了这些问题。