# Testing Strategy

## Goal

建立统一、低波动、可复用的测试覆盖方法，保证 feature 变更可以：

- 快速验证（短反馈周期）
- 离线验证（不依赖真实网络/backend）
- 回归可控（覆盖成功路径 + 失败不变性）

本仓库的行为场景表达约束与映射方法，见 `docs/codex/bdd-methodology.md`。

## Hard Constraints

- 不引入新的业务逻辑语义。
- 不做大规模 Service / Domain 重构，优先最小改动。
- 测试默认离线运行，使用 fake/mock 与本地临时数据。
- 失败路径必须验证不变性（DB/状态不被污染）。
- 真实 backend、真实账号、真实链 RPC 测试只能作为 smoke/live 测试显式运行。

## Test Layers

### Unit

单元测试只验证一个函数、一个规则或一个步骤。
表达风格使用 Arrange-Act-Assert，保持直接。

允许：

- 纯函数、状态判断、序列化/反序列化、错误映射。
- 手写轻量 fixture。
- `#[test]` 或不触网的 `#[tokio::test]`。

禁止：

- 真实网络请求。
- 真实 backend、真实链 RPC、真实 OSS。
- 固定 `test_data`。
- `WalletManager::new()` 或全局 `CONTEXT` 初始化。

模板：

```rust
#[test]
fn diagnose_withdraw_should_wait_for_audit_when_tx_ack_sent() {
    let withdraw = WithdrawFixture::new()
        .tx_ack_sent()
        .without_audit_result()
        .build();

    let diag = diagnose_withdraw(&withdraw);

    assert_eq!(diag.stage, AdvancementPoint::CanBuild);
    assert_eq!(diag.next_expected_fact, Some("audit_passed_at"));
}
```

### Component

组件测试验证一个模块与本地依赖的协作，例如 DAO/repository/domain + SQLite。
表达风格使用 Arrange-Act-Assert；如果 DB 准备很吵，可以用少量
`given_*` helper，但不要依赖 integration harness。

允许：

- `tempfile::TempDir` 或唯一临时目录。
- 真实 SQLite migration。
- 真实 DAO/repository。
- 断言真实 DB 状态。

禁止：

- 真实网络请求。
- 依赖测试执行顺序。
- 多个测试共享同一个 DB 文件或 `test_data` 目录。

模板：

```rust
#[tokio::test]
async fn list_running_by_uid_returns_only_running() {
    let db = TestDb::new_api_wallet().await;

    db.insert_address_query("u1", "eth", AddressQueryStatus::Running).await;
    db.insert_address_query("u1", "bsc", AddressQueryStatus::Done).await;

    let rows = AddressQueryStateDao::list_running_by_uid(db.pool(), "u1")
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].chain_code, "eth");
    assert_eq!(rows[0].status, AddressQueryStatus::Running);
}
```

### Integration

标准集成测试验证一条业务流程中多个模块的协作。它不是 live 测试，不依赖真实 backend 或真实链 RPC。

集成测试统一使用 Given-When-Then 业务剧本风格：

- `given_*`: 准备业务事实、DB 事实、fake/mock backend、fake/mock chain、
  通知收集器。
- `when_*`: 执行一个业务入口、worker step、scanner step 或 retry。
- `then_*`: 断言返回值、DB、backend 调用、通知、task queue、scanner
  label、幂等或重试结果。

这不是否定 Arrange-Act-Assert；Given-When-Then 是 integration 层对 AAA
的业务化命名。Unit、component、smoke/live 仍使用 AAA，integration 用业务剧本
提高可读性。

模板：

```rust
#[serial]
#[tokio::test]
async fn withdraw_tx_ack_backend_failure_keeps_fact_unset_and_retryable() {
    let scenario = WithdrawNotificationScenario::new().await;
    let order = WithdrawOrderFixture::new("withdraw_ack_fail");

    scenario.given_withdraw_order(&order).await;
    scenario.given_backend_next_ack_fails(503, "ack unavailable");

    let result = scenario.when_tx_ack_is_sent(&order).await;

    then_worker_left_flow_retryable(result);
    scenario.then_backend_tx_ack_attempted_once(&order).await;
    scenario.then_tx_ack_fact_is_not_persisted(&order).await;
    scenario.then_scanner_can_retry_tx_ack(&order).await;
}
```

集成测试正文应该避免直接暴露底层噪音，例如：

- `SqliteContext`
- `serde_json::to_value`
- notification channel 创建
- backend recorder 解密
- repo 查询细节

这些可以放在 scenario 私有 helper 中，但测试正文要保留业务含义。

集成测试优先覆盖：

- 成功路径：状态、事实字段、返回值正确。
- 参数失败：非法地址、错误密码、缺失字段、余额不足。
- 后端失败：业务错误码、非成功响应、超时、加解密失败。
- 链上失败：构建失败、广播失败、确认失败。
- 幂等重试：重复消息、重复 ACK、重复 receipt 上传。
- 恢复路径：进程中断后根据 DB 事实字段继续。
- 并发路径：同一 `trade_no` 多任务竞争时只能执行一次关键副作用。
- DB 事务：失败时不留下半状态。
- 通知/副作用：前端通知、backend call、task queue 都可断言。

### Smoke / Live

Smoke/live 测试用于验证真实环境联通性，有价值但不能作为默认测试。

要求：

- 允许真实 backend、真实链 RPC、真实账号状态。
- 测试名或模块名包含 `live` 或 `smoke`。
- 使用独立 feature 或 `#[ignore]` 手动运行。
- 不打印私钥、助记词、生产凭据或生产配置。

## Directory Layout Standard

目录结构必须让读代码的人直接看出三件事：

- 测试层级：unit / component / integration / smoke-live
- 业务模块：withdraw / collect / fee / transaction / stake 等
- 具体 flow：confirm / resource gate / ack / worker / live backend 等

### Source-side Unit / Component Tests

适用场景：

- 需要访问 private / `pub(crate)` 函数。
- 只测一个函数、一个状态转换、一个 repo/dao 事实写入。
- 不需要 fake backend、fake chain 或完整 `WalletManager`。

少量测试可以留在文件底部：

```text
wallet-api/src/domain/api_wallet/trans/withdraw.rs
```

当测试超过 3 个，或同一模块出现多个 flow，必须拆到同名目录：

```text
wallet-api/src/domain/api_wallet/trans/
  withdraw.rs
  withdraw/
    confirm_tests.rs
    audit_tests.rs
    resource_gate_tests.rs
```

在 `withdraw.rs` 中只保留测试模块入口：

```rust
#[cfg(test)]
mod confirm_tests;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod resource_gate_tests;
```

如果 Rust 模块路径不适合嵌套，也可以保留同级 `*_tests.rs`，但文件名必须说明 flow：

```text
wallet-api/src/domain/api_wallet/trans/
  confirm_tx_tests.rs
```

### Crate-level Integration / Smoke Tests

Integration 和 smoke/live 不放在 `src/`，统一放在 crate 的 `tests/` 下。

推荐结构：

```text
wallet-api/
  tests/
    harness/
      mod.rs
      worker.rs
      fixtures.rs
      fake_backend.rs
      fake_chain.rs
      assertions.rs
    integration/
      mod.rs
      api_wallet/
        mod.rs
        withdraw_resource_gate.rs
        withdraw_confirm.rs
        collect_receipt/
          mod.rs
          support.rs
        collect_worker.rs
        fee_worker.rs
      transaction/
        mod.rs
        transfer.rs
        nonce.rs
      stake/
        mod.rs
        tron_stake.rs
    smoke/
      mod.rs
      live_backend.rs
      live_chain.rs
```

`wallet-api` 当前 `api_wallet` 模块推荐按 flow 拆分：

```text
wallet-api/
  tests/
    integration/
      api_wallet/
        mod.rs
        withdraw_resource_gate.rs     # 提币资源门控/资源 ACK
        withdraw_notification.rs      # 提币通知、TX ACK、副作用幂等
        collect_resource_gate.rs      # 归集资源门控/资源 receipt
        collect_receipt/              # 归集交易执行 receipt
          mod.rs                      # read-first 测试用例
          support.rs                  # flow-local fixture/scenario/helper
        collect_fee_cycle.rs          # 归集 fee-cycle scanner
        collect_fee.rs                # 归集手续费/服务费流程
        collect_notification.rs       # 归集通知重试
        collect_recovery.rs           # 归集恢复/重建路径
        collect_local_reclaim.rs      # 归集本地回收路径
```

拆分原则：

- 文件名回答“这是什么 flow”，而不是回答“它从哪个旧文件迁来”。
- 一个文件可以包含同一 flow 的成功、失败、幂等、恢复用例。
- 简单 flow 使用 `<flow>.rs`；复杂 flow 使用 `<flow>/mod.rs` +
  `<flow>/support.rs`，让 `mod.rs` 成为第一阅读入口。
- `<flow>/mod.rs` 只放测试用例和少量导入；`support.rs` 放本 flow 私有的
  fixture、scenario、SQL、backend recorder、payload 和 assertion 细节。
- `support.rs` 不是共享层；只有跨两个以上 flow 复用的环境能力才上移到
  `tests/harness`。
- 不同副作用边界应拆开，例如 notification、backend ACK、receipt、fee。
- 旧测试迁移时优先改文件归属和命名，业务行为保持不变。

`wallet-database` 推荐结构：

```text
wallet-database/
  tests/
    harness/
      sqlite.rs
      assertions.rs
    component/
      api_wallet/
        withdraw_repo.rs
        collect_repo.rs
        fee_repo.rs
      migrations/
        api_transaction_schema.rs
```

过渡期允许保留当前 `tests/<module>/mod.rs` 结构，但新增文件应按以下规则命名：

- `tests/integration/<module>/<flow>.rs`：标准集成测试。
- `tests/integration/<module>/<flow>/mod.rs`：复杂 flow 的 read-first 测试入口。
- `tests/integration/<module>/<flow>/support.rs`：复杂 flow 的本地测试支撑。
- `tests/smoke/<module>/<flow>.rs`：真实环境 smoke/live。
- `tests/harness/<capability>.rs`：跨模块 test harness 能力。
- `src/.../<module>/<flow>_tests.rs`：贴近源码的 unit/component 测试。

### Helper Ownership

- 模块私有 helper 只能服务本模块，放在同模块测试目录。
- 跨模块 helper 才能进入 `tests/harness/`。
- 如果 `withdraw` 需要复用 `collect` 里的 helper，应先把 helper 上移到 `tests/harness/`，再由两个模块共同依赖。
- helper 只能做数据准备、fake 配置、结果断言，不承载业务决策。

## Default Commands

日常默认验证优先运行稳定测试：

```bash
cargo fmt --all
cargo check
cargo test --workspace --no-default-features
```

受影响 crate 测试：

```bash
cargo test -p wallet-database
cargo test -p wallet-api --no-default-features
cargo test -p wallet-transport-backend --no-default-features
```

标准集成测试显式运行：

```bash
cargo test -p wallet-api --no-default-features --features integration-tests
```

真实环境 smoke/live 测试显式运行：

```bash
cargo test -p wallet-transport-backend --features live-smoke -- --ignored --nocapture
```

过渡期注意：

- 当前 `wallet-api` default feature 仍包含 `integration-tests`。
- 在 feature 拆分完成前，不要把普通 `cargo test -p wallet-api` 视为纯单元测试。
- 新增测试先按本文档分类；旧测试后续逐步迁移。

## Standard Iteration Model

### Iteration 0 — Baseline Stability

- 固定命令与执行入口（本地/CI一致）。
- 固定隔离策略（serial / OnceCell / task noop）。
- 消除随机性断言（不直接断言时间戳与随机值）。

### Iteration 1 — Golden Path

- 先覆盖 1 条核心成功链路。
- 完成最小断言矩阵：DB 变化 + backend 调用记录。

### Iteration 2 — Error Paths

- 每条 flow 至少 1 条失败路径。
- 优先覆盖 backend error / not found / status mismatch / binding 缺失。

### Iteration 3 — Orchestration Regression

- 锁定调用顺序、调用次数、远端/本地先后顺序。
- 至少 2 个“失败不落库”或“边界顺序”回归用例。

### Iteration 4 — Lightweight Domain Forwarding (Optional)

- 仅验证请求组装与转发逻辑。
- 强 DB 行为优先由 service/integration tests 覆盖。

## Test Case Template

- 用例名：
- 测试层级：unit / component / integration / smoke-live
- 入口函数：
- 前置数据/夹具：
- fake/mock 配置：
- 执行步骤（arrange / act / assert）：
- 断言（至少 DB 状态 + backend 调用记录）：
- 覆盖分支/风险点：
- 预计改动范围：

## Assertion Matrix Template

每条 flow 按以下字段记录：

- Flow：示例 flow
- 输入组合：关键参数组合
- 预期 backend 调用：接口、次数、关键字段
- 预期 DB 变化：表、字段、状态
- 失败不变性：失败时必须保持不变的字段

填写原则：

- 先写成功路径，再写失败路径。
- 每条失败路径明确“不变字段”。

## Assertion Rules

新增测试必须至少有一个业务断言。

允许：

- `assert!`
- `assert_eq!`
- `matches!`
- 自定义 assertion helper
- 查询 DB 后断言字段
- 断言 fake/mock backend 的调用记录

不允许只做：

```rust
println!("{res:?}");
tracing::info!("res: {res:?}");
Ok(())
```

如果测试只验证真实环境能否连通，应归入 smoke/live。

## Fixture / Helper Standard

- `ensure_env`: 构建并复用测试环境
- `prepare_*`: 准备测试数据
- `reset_fake`: 每个测试前重置 fake 状态
- `snapshot_*`: 采集前后快照
- `assert_*_call`: 调用次数与字段断言

约束：helper 只做数据准备与断言支持，不承载业务判断。

新增 fixture 还必须遵守：

- 数据只服务当前测试，不依赖历史执行结果。
- 使用唯一 `trade_no`、`uid`、地址或临时目录。
- DB fixture 必须由测试自己创建和销毁。
- 不在测试中写固定 `test_data`，除非该测试明确归为 smoke/live。
- 不使用真实私钥、助记词、生产凭据。

## API Wallet Example (Reference)

对于 `import_api_wallet` / `scan_bind` / `import_bind` 这类流程：

- 默认使用 `FakeApiWalletBackend + temp sqlite + serial + task noop`
- 核心断言包括：
  - wallet relation / app_id / merchant_id / sn 等关键字段落库
  - backend 调用接口、次数与请求字段准确
  - 失败路径下字段保持不变

交易状态、事实字段、ACK、receipt、重试、恢复相关改动必须断言 DB 字段。

常见事实字段包括：

- `tx_ack_sent_at`
- `audit_passed_at`
- `audit_rejected_at`
- `raw_tx`
- `tx_hash`
- `last_broadcast_at`
- `transaction_time`
- `tx_exec_receipt_uploaded_at`
- `tx_res_received_at`
- `tx_res_ack_sent_at`
- `finished_at`
- `chain_success_at`
- `chain_failed_at`

## Legacy Test Migration

不要一次性重写所有旧测试，按以下顺序迁移：

1. 新增测试先遵守本文档。
2. 修 bug 时，把对应旧测试改成可断言的回归测试。
3. 对只 `println!/tracing` 的测试分类：
   - 能本地断言的，迁移到 unit/component/integration。
   - 必须真实后端的，迁移到 smoke/live。
4. 最后再逐步拆分 default feature，避免普通 `cargo test` 自动跑 live 或不稳定测试。

## Definition of Done

- 基线 smoke 连续执行 3 次稳定通过（建议）
- 每条关键 flow 至少 1 条错误路径测试
- 至少 2 个编排回归测试（适用时）
- 断言矩阵已更新
- 不依赖真实网络/backend
- 没有打印敏感信息。
