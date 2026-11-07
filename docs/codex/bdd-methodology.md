# BDD 方法论与项目实践（wallet_api 为准）

本文给出一个“能直接照着用”的 BDD 使用约定：先讲清楚团队口径，再给可落地示例。

## 1. 基本定义

- **BDD（Behavior-Driven Development）**：先定义行为，再驱动实现与验证。
- **GWT / Given-When-Then**：BDD 常用的表达方式，描述一个场景的前后关系。
- **Gherkin**：描述业务场景的文本语法（例如 `Feature`、`Scenario`、`Given`、`When`、`Then`）。

在这个项目里：

- 严格按“完整 BDD 流程”只在特定流程引入；
- 当前默认要求是：**测试正文按 Given-When-Then 的行为剧本写法**（主要用于 integration tests）；
- 因此你会看到“`GWT-heavy`”，它通常只表示“测试风格重度偏 GWT”，不是“全流程 BDD”。

## 2. 与严格 BDD 的关系（简版）

**严格 BDD 需要做到的核心**：

- 行为约束要和产品/测试共识写成同一种语言；
- 场景（需求）先于实现；
- 场景自动化有稳定执行入口（不是只写在脑子里）；
- 回归按行为场景连续验证。

**当前项目要求**（更实用）：

- 测试层按 `unit / component / integration / smoke-live` 分层；
- integration 的正文优先使用 `Given-When-Then`；
- 每个关键 flow 至少有成功 + 失败不变性；
- 先确保离线可验证，再决定是否放 `smoke/live`。

## 3. 场景模板（直接可套）

```gherkin
Feature: <业务特性>
  Rule: <可选，描述一条业务规则>

  Scenario: <场景名称 - 正常路径>
    Given <前置状态1>
    And <前置状态2>
    When <触发动作>
    Then <可观测结果1>
    And <可观测结果2>
```

失败场景同理：

```gherkin
  Scenario: <场景名称 - 失败路径>
    Given <前置状态>
    And <失败触发条件>
    When <同样动作>
    Then <返回错误/可重试>
    And <关键状态不变>
    And <副作用调用次数可见>
```

## 4. 项目里的“可直接实现”示例

示例来源：提现通知/ACK 流程（可对应 `wallet-api/tests/integration/api_wallet/withdraw_notification/`）。

```gherkin
Feature: API Wallet 提现 TX ACK 行为
  Scenario: TX ACK 成功后写 ACK 事实
    Given 存在一条提现订单（trade_no = T_WS_OK），状态是 AuditPass
    And fake backend 已准备一次 TRANS_EVENT_ACK 成功响应
    When 执行 tx_ack worker 一次
    Then 返回成功
    And backend 收到 1 次 TRANS_EVENT_ACK（type=WD, ack_type=TX）
    And 订单 tx_ack_sent_at 被持久化
    And scanner 不再重复发 SendTxAck

  Scenario: TX ACK 失败时保持可重试
    Given 存在一条提现订单（trade_no = T_WS_FAIL），状态是 AuditPass
    And fake backend 下一次返回 503 "ack unavailable"
    When 执行 tx_ack worker 一次
    Then 返回可重试错误
    And backend 收到 1 次 TRANS_EVENT_ACK（type=WD, ack_type=TX）
    And 订单 tx_ack_sent_at 保持 NULL
    And scanner 可再次进入 SendTxAck 重试
```

## 5. 从 Gherkin 场景落到 Rust 测试的规则

- 每个 `Given` 对应 `scenario.given().xxx`；
- 每个 `When` 对应 `scenario.when().xxx`；
- 每个 `Then` 对应 `scenario.then().xxx`；
- `Given/When/Then` 里只做行为断言，不在 `mod.rs` 里写数据库 SQL、mock 解析细节、transport 解密细节；
- 如果细节多，拆 `support.rs`。

### 5.1 最小实现模板（代码侧）

```rust
#[serial]
#[tokio::test]
async fn withdraw_tx_ack_backend_failure_keeps_fact_unset_and_retryable() {
    let scenario = WithdrawNotificationScenario::new().await;
    let fixture = WithdrawScenarioFixture::new("backend_fail");

    scenario.given_withdraw_order(&fixture).await;
    scenario.given_backend_ack_fails(503, "ack unavailable");

    let result = scenario.when_tx_ack_runs(&fixture).await;

    scenario.then_result_is_retryable(result);
    scenario.then_backend_ack_called_once(&fixture, "WD", "TX").await;
    scenario.then_tx_ack_fact_is_not_set(&fixture).await;
    scenario.then_scanner_can_retry_send_tx_ack(&fixture).await;
}
```

> 上面是行为映射示例，类型名可按项目现有命名替换。

## 6. 本地执行与验收检查清单

- 先写 1 个成功场景，再补 1 个失败不变性场景；
- 失败场景至少包含“可重试/可恢复 + 关键事实不变”；
- 每条主 flow 至少一个幂等/重试断言；
- 触发外部依赖都要有 fake 或明确定义；
- 更新 `docs/codex/assertion-matrices/*`。

## 7. 与现有规范的关系

- 本文属于 `docs/codex/testing*.md` 的执行约束补充；
- 规范主链路仍以 `docs/codex/testing.md` 与 `docs/codex/testing-strategy.md` 为准；
- 断言矩阵仍按 `docs/codex/assertion-matrices/` 维护。
