# Testing Rules (Codex Quick Reference)

## Purpose

本文件是测试规则摘要，供 Codex 在执行任务时优先读取。
它是入口，不是唯一规范。需要细节时按下面顺序继续读：

1. `docs/codex/testing-strategy.md`：完整分层、目录、命令和迭代策略。
2. `docs/codex/testing-templates.md`：可复制的 test/scenario/support 模板。
3. `docs/codex/testing-backend-boundaries.md`：backend fake 边界和复杂 flow fake 设计。
4. `docs/codex/testing-backend-fake-inventory.md`：当前 fake 覆盖状态和迁移优先级。
5. `docs/codex/bdd-methodology.md`：Given-When-Then 与项目实践的关系。
6. `docs/codex/assertion-matrices/`：各高风险 flow 的断言账本。

## Required Rules

- 测试改动必须遵循最小改动原则，不引入新的业务语义。
- 测试必须默认离线可运行，不依赖真实 backend/真实网络。
- 测试必须按 unit / component / integration / smoke-live 分层，不能把真实环境测试混入默认测试。
- 归集、提币、手续费、资源等 API wallet transaction-like flow 改动必须先按
  `docs/codex/api-wallet-flow-maintenance.md` 确认生命周期阶段和不变性。
- 每次功能改动必须新增或更新测试（至少覆盖一条成功路径）。
- 每个关键 flow 必须至少有一条失败路径测试，并断言“不变性”。
- 必须更新断言矩阵（涉及 flow 改动时）。
- 仅运行受影响测试命令；非必要不跑全量。
- 表达风格必须按层选择：Unit / Component / Smoke 使用
  Arrange-Act-Assert；Integration 使用 Given-When-Then 业务剧本。

## Layer Rules

- Unit：只测一个函数、规则或步骤；禁止真实网络、固定 `test_data`、`WalletManager::new()`、全局 `CONTEXT`。
- Component：允许临时 SQLite + migration + 真实 repo/dao；禁止真实 backend/链 RPC；必须断言真实落库结果。
- Integration：使用 fake/mock backend、fake/mock chain、本地临时数据和统一 fixture；测试正文必须用
  Given-When-Then 表达业务场景。简单 flow 可用 `given_*` / `when_*` /
  `then_*`；复杂 flow 优先用 `scenario.given()` / `scenario.when()` /
  `scenario.then()` role API，并断言返回值、DB、外部调用和副作用。
- 复杂 backend flow：禁止为每个测试场景写一套专用 fake；必须 fake 接口边界，
  并用响应队列、调用记录、错误/延迟注入和未配置调用可见化来表达场景差异。
  先看 `testing-backend-boundaries.md`，再从 `testing-templates.md` 复制脚本模板。
- Smoke/Live：允许真实 backend/真实链路，但必须显式标记并手动运行，默认测试和 CI 主链不运行。

## Style Rules

- Unit：使用 Arrange-Act-Assert；保持直接，不引入 integration harness。
- Component：使用 Arrange-Act-Assert；可用少量 `given_*` helper 减少 DB
  准备噪音，但测试正文仍要能看到被测模块和核心断言。
- Integration：使用 Given-When-Then。测试正文应像业务剧本：
  `given` 准备业务事实或 fake 行为，`when` 执行一个业务入口或 worker
  step，`then` 断言 DB、backend、通知、scanner、幂等或重试结果。复杂
  flow 推荐 `tests/harness` 的通用 role 容器加 flow-local trait，例如
  `scenario.given().blocked_order(...)`。业务 trait 留在本 flow，公共 role
  容器可以复用。
- Smoke/Live：使用 Arrange-Act-Assert；保持手工验证步骤直白，必须
  `#[ignore]` 或独立 feature。
- Integration test body 禁止直接暴露低层噪音，例如 `SqliteContext`、
  `serde_json::to_value`、backend recorder 解密、channel 创建。低层动作可以在
  scenario 私有 helper 中出现。

## Directory Rules

- 路径必须表达测试层级、业务模块、具体 flow；不要把多类测试混在一个大 `mod.rs`。
- Unit / component 默认贴近被测代码；当一个模块超过 3 个测试或出现多个 flow 时，拆到同名目录下的 `*_tests.rs`。
- Integration / smoke 统一放在 crate 的 `tests/` 下，
  按 `integration/<module>/`、`smoke/<module>/` 拆分。
- 简单 flow 可以使用 `integration/<module>/<flow>.rs`；如果一个 flow
  需要较多 fixture、scenario、SQL、backend recorder、payload 或 assertion
  细节，必须拆成 `integration/<module>/<flow>/mod.rs` +
  `integration/<module>/<flow>/support.rs`。
- `<flow>/mod.rs` 是 read-first 测试入口，只放测试用例和少量导入；
  `<flow>/support.rs` 只放本 flow 私有的 fixture、scenario 和低层 helper。
- `tests/harness/` 只放真正跨模块复用的 test harness、fake、fixture、assertion、
  通用 role 容器；禁止 `withdraw` 依赖 `collect` 私有 helper。
- `src/testkit/` 只放 crate 内部测试入口，例如 worker/scanner/domain
  step wrappers；它不是测试环境，也不是通用 helper 垃圾桶。
- 新增测试优先用清晰文件名表达意图，例如 `confirm_tests.rs`、`resource_gate.rs`、`live_backend.rs`。

目录契约：

```text
<crate>/
  src/...                         # unit / component tests live near code
  src/testkit/                    # crate-side test-only internal entrypoints
  tests/
    harness/                      # cross-module test environment and fakes
    integration/<business_module>/<flow>.rs
    integration/<business_module>/<flow>/
      mod.rs                      # read-first test cases
      support.rs                  # flow-local fixture/scenario/helpers
    smoke/<business_module>/<flow>.rs
```

命名契约：

- `<business_module>` 使用业务归属，例如 `api_wallet`、`transaction`、`stake`。
- `<flow>.rs` 或 `<flow>/mod.rs` 使用具体流程或风险点，例如
  `withdraw_notification.rs`、`collect_resource_gate.rs`、`collect_receipt/mod.rs`。
- 一个 integration 文件只覆盖同一类业务流程；如果同时出现 fee、notification、receipt，应拆成多个文件。
- `support.rs` 是 flow-local 测试支撑文件，不是跨 flow 共享层；只有至少两个
  flow 复用的环境能力才能上移到 `tests/harness`。
- `harness` 是专业测试术语，表示测试夹具/执行环境总成；它只能承载通用环境、fake、fixture、assertion，不能变成新的业务模块。
- `testkit` 表示 crate 内部测试工具包，只暴露测试需要的内部步骤入口；
  integration 测试环境仍然归 `tests/harness`。

## Required Process

1. 先确认改动边界与受影响 flow。
2. 对 API wallet transaction-like flow，先确认触及的生命周期阶段、durable
   fact、side effect owner 和不变性。
3. 先补黄金路径，再补失败路径。
4. 用 fixture/scenario/helper 复用测试准备逻辑；integration 测试正文使用
   Given-When-Then。
5. 记录并汇报执行的测试命令与结果摘要。
6. 按 `docs/codex/checklists/pr-definition-of-done.md` 逐项完成 DoD。

## Command Policy

- 优先运行最小验证集合（受影响 crate/test target）。
- 若需要扩展验证，按风险逐步扩大范围。
- 默认稳定测试优先使用 `cargo test --workspace --no-default-features`。
- 标准集成测试显式使用 `--features integration-tests`。
- 真实环境 smoke/live 测试必须使用独立 feature 或 `#[ignore]`。

过渡期注意：当前 `wallet-api` default feature 仍包含 `integration-tests`。
在拆分完成前，不要把普通 `cargo test -p wallet-api` 视为纯单元测试。

## Large-Repo Kickoff (Low Token)

### Pilot Module First

- 大仓库默认先选 1 个高风险模块做样板工程，默认从 `wallet-api` 开始。
- 选择标准：改动频繁 / 线上风险高 / 现有测试薄弱（满足任意两条）。

### Per-Round Limits

- 每轮只处理一个模块、一个 flow。
- 每轮最多 3 个可执行子任务。
- 非目标代码不做“顺便优化”。

## Iteration Order

### Iteration 0 — Quality Gate

- 固定模块级最小命令（smoke + crate check）。
- 固定 PR DoD 勾选流程。
- 清理/忽略真实网络依赖测试（手工运行单独标记）。

### Iteration 1 — Minimal Closure

- 每条 flow 先补：
  - `happy_path_ok`
  - `error_path_no_side_effect`
- 必须包含 DB 状态 + 外部调用记录双断言。

### Iteration 2 — Orchestration Guards

- 补 `orchestration_order_guard` 类型用例。
- 至少新增 2 个“顺序/次数/失败不落库”回归用例。

### Iteration 3 — Reusable Test Infra

- 抽取模块内 `tests/harness` fixture/helper。
- 形成可复制模板，用于下一条 flow。

### Iteration 4 — Scale to Next Module

- 同一模板迁移到第二个高风险模块。
- 保持“每轮仅一个模块”策略。

## Round Validation Metrics

每轮只看 4 个指标：

1. 目标 flow 是否有成功+失败覆盖
2. 是否离线稳定跑通（建议连续 3 次）
3. 是否存在不变性断言
4. 是否通过 PR DoD checklist
