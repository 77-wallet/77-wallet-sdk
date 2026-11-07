# Testing Backend Boundaries

## Purpose

本文档记录 `wallet-api` 集成测试里的外部 backend 边界。
目标不是描述所有测试细节，而是回答三个问题：

- 哪些调用已经可以被 fake backend 控制。
- 哪些调用仍然可能绕过 fake backend。
- 后续新增或迁移测试时，应如何扩展 fake backend。

当前 fake 覆盖清单见 `docs/codex/testing-backend-fake-inventory.md`。

## Core Rule

标准集成测试必须离线可运行。
测试可以调用真实业务入口，例如 `WalletManager` 或 worker step，但这些入口
背后的外部依赖必须被 fake、mock 或本地测试资源接管。

也就是说：

- 业务流程是真的。
- 本地 DB 和测试数据是真的。
- backend、链 RPC、通知通道等外部世界必须是假的或可记录的。

## Current Api-Wallet Boundary

`wallet-api` 目前已有一条比较清楚的 API wallet backend 边界：

- Production boundary:
  `wallet-api/src/context/api_wallet_backend.rs`
- Test fake:
  `wallet-api/tests/harness/mod.rs`
- Test injection:
  `WalletManager::new_for_test(...)`

`ApiWalletBackend` 当前覆盖这些接口：

- `wallet_bind_appid`
- `init_api_wallet`
- `old_keys_init`
- `appid_import`
- `appid_import_recharge_wallet`
- `keys_uid_check`
- `query_uid_bind_info`
- `query_wallet_activation_info`
- `appid_uid_usage`

对应的 `FakeApiWalletBackend` 已经实现这些接口，并提供三类能力：

- 返回值队列，例如 uid 状态、绑定信息、使用情况。
- 错误注入，例如绑定失败、导入失败、查询失败。
- 调用记录，例如绑定参数、导入参数、uid 查询和激活信息查询参数。

这条边界适合 `import_bind` 这类集成测试。测试调用 manager 方法时，业务代码
会走 trait，测试环境注入 fake，因此不会访问真实 backend。

## Current Gaps

目前还没有做到所有 backend 调用都统一经过 `ApiWalletBackend`。
代码里仍有大量直接访问：

```rust
CONTEXT.get().unwrap().get_global_backend_api()
```

这类调用拿到的是 `wallet_transport_backend::api::BackendApi`，不是当前
`FakeApiWalletBackend`。如果某个集成测试触发了这条路径，测试就可能依赖真实
backend 配置，或者只能靠另一个 worker fake 去截获请求。

需要特别注意的区域：

- `wallet-api/src/application/api_wallet_withdraw.rs`
  withdraw orchestration 当前直接使用 global backend。
- `wallet-api/src/infrastructure/api_trans/**`
  collect、withdraw、fee、resource operation、resource reclaim 的 worker
  里还有多处 global backend 调用。
- `wallet-api/src/service/**`
  app、wallet、multisig、jpush 等 service 仍有多处 global backend 调用。
- `wallet-api/src/domain/assets/mod.rs`
  assets 查询和同步路径仍有 global backend 调用。

因此，现在的结论是：

- `import_bind` 与 activation info 的部分测试边界较清楚。
- 整个 `wallet-api` 还没有完成统一 backend fake 化。
- 后续扩展集成测试时，不能默认认为所有 manager 调用都已经离线可信。

## Design Standard

新增或迁移 backend 相关集成测试时，按下面规则处理。

### 1. Define A Boundary Before Writing Scenarios

先确认被测 flow 会触发哪些外部接口。
如果接口还在走 `get_global_backend_api()`，不要直接写场景断言。
先决定本轮是否要：

- 把该接口纳入一个 trait boundary。
- 或者确认已有 worker-level fake 能稳定截获该请求。
- 或者把用例降级为 `smoke/live`，手动运行。

### 2. Fake The Interface, Not The Scenario

fake backend 应该对应业务接口，而不是对应单个测试场景。

推荐：

```text
ApiWalletBackend
  FakeApiWalletBackend
    records calls
    queues configured responses
    injects scoped errors or delays
```

不推荐：

```text
fake_scan_bind_success_for_test_a
fake_scan_bind_timeout_for_test_b
fake_scan_bind_error_for_test_c
```

场景差异应通过配置 fake 的返回、错误、延迟、调用记录来表达。

### 3. Unconfigured Calls Should Be Visible

fake backend 不应该悄悄返回过于宽松的默认值。
如果某个接口在测试中没有被配置，但业务代码调用了它，测试应能暴露这个事实。

可接受的方式：

- 明确 panic，提示哪个接口没有配置。
- 返回明确测试错误。
- 记录调用并由 `then` 断言调用次数和参数。

### 4. Scoped Network Behavior

网络特征应该按 backend 行为命名，而不是按某个业务接口命名。

推荐：

```rust
let _network = given.backend_network_is_slow();
```

不推荐：

```rust
let _delay = given.appid_import_delay();
```

原因是慢请求、超时、重试、乱序响应是网络接口的通用特征。
这些能力成熟后，可以抽到 harness 的通用 backend fake 配置里。

### 5. Assert Both Result And Boundary

集成测试不能只断言返回值。
凡是触发 backend 的 flow，至少要断言：

- 业务返回值。
- DB 状态或事实字段。
- backend 调用次数。
- backend 调用参数。
- 失败路径下不应发生的调用。

例如 `scan_bind` 的 remote-first 场景，不只断言结果是错误，还要断言 fake
backend 确实被调用一次，并且参数是预期的钱包对和 app id。

## Complex Flow Fake Design

复杂接口通常包含多个 worker step、多次 backend 调用、多条失败路径和重试状态。
这种 flow 不应按测试用例复制 fake，而应拆成“接口级 fake + 场景脚本 +
flow-local 业务 facade”三层。

### 1. Normalize The External Boundary

先列出被测 flow 会触发的所有外部接口，并确认每个接口属于哪个生产边界。
如果某个调用仍然直接访问 `get_global_backend_api()`，本轮必须先做出选择：

- 把该调用纳入已有 trait，例如 `ApiWalletBackend`。
- 或新增一个按外部系统命名的 trait boundary。
- 或确认已有 worker-level recorder/fake 可以稳定截获它。
- 或把该用例归入 `smoke/live`，不放入默认 integration。

复杂 flow 的测试入口可以调用真实业务方法或真实 worker step，但所有远端依赖
必须能被 fake、recorder 或本地资源接管。

### 2. Use A Scriptable Fake Shape

复杂 fake 至少应具备四类能力：

- response queue：按调用顺序返回成功、业务错误、超时或特定 payload。
- call recorder：记录接口名、次数、关键请求字段和顺序。
- scoped behavior：按接口或网络行为注入错误、延迟、乱序或重试响应。
- unconfigured visibility：未配置调用必须 panic、返回明确测试错误，或被记录后由
  `then` 断言暴露。

推荐形态：

```text
BackendTrait
  FakeBackend
    queues configured responses
    records every call
    injects scoped errors or delays
    exposes unread calls and unconfigured calls
```

场景差异通过配置 fake 表达，不通过新增 `fake_xxx_for_case_y` 表达。

### 3. Keep Flow Logic In Scenario Facades

测试正文只描述业务剧本；复杂 fake 的队列、payload、解密、SQL 和 recorder
细节必须藏在 flow-local `support` 中。

推荐正文形态：

```rust
let scenario = ComplexFlowScenario::new().await;
let fixture = ComplexFlowFixture::new("backend_fail_after_broadcast");
let given = scenario.given();
let when = scenario.when();
let then = scenario.then();

given.pending_order(&fixture).await;
given.backend_script(&fixture)
    .fee_quote_ok()
    .build_tx_ok()
    .broadcast_fails(503, "temporary backend error");

let result = when.worker_step_runs(&fixture).await;

then.step_is_retryable(result);
then.backend_calls_match_script(&fixture).await;
then.broadcast_fact_is_not_persisted(&fixture).await;
then.scanner_can_retry(&fixture).await;
```

`given.backend_script(...)` 可以是 flow-local builder；它负责把人能读懂的业务步骤
翻译成 fake backend 的响应队列和预期调用。

### 4. Split By Business Step, Not By Test Case

如果一个接口流程包含 `quote -> build -> sign -> broadcast -> ack -> receipt`
这类步骤，优先按业务步骤组织 fake 能力：

- `fee_quote_ok()` / `fee_quote_fails(...)`
- `build_tx_ok()` / `build_tx_fails(...)`
- `broadcast_ok()` / `broadcast_fails(...)`
- `tx_ack_ok()` / `tx_ack_fails(...)`
- `receipt_upload_ok()` / `receipt_upload_fails(...)`

不要按用例组织：

```text
fake_happy_path()
fake_backend_fail_case_1()
fake_backend_fail_case_2()
```

按步骤组织后，成功路径、任意中间失败、幂等重试和恢复路径都可以用同一个 fake
组合出来。

### 5. Assert The Contract, Not Just The Result

复杂 flow 的 `then` 至少覆盖：

- 业务返回值或 worker 结果。
- DB fact/status 的最终状态。
- backend 调用次数、顺序和关键字段。
- 失败路径下未发生的调用。
- 失败路径的不变性，例如未写入 durable fact、未重复通知、未重复 ACK。
- 重试或恢复路径是否仍可由 scanner/worker 继续推进。

如果只能断言“没有报错”，这个用例还不够成为 integration regression。

### 6. Grow Fakes Incrementally

不要一次性 fake 完一个大系统。每轮只覆盖目标 flow 真实触发的接口：

1. 先写成功路径需要的最小响应队列和调用记录。
2. 再补一个最重要失败点的不变性断言。
3. 当第二个 flow 需要同样能力时，再把 flow-local helper 上移到
   `tests/harness`。
4. 每次新增 backend 能力，都同步更新对应 assertion matrix。

这能避免 fake 本身变成另一个难维护的业务系统。

## Migration Order

不要一次性把所有 `get_global_backend_api()` 都改掉。
按 flow 迁移更安全：

1. 保持 `import_bind` 作为当前参考样板。
2. 把通用 error、delay、call assertion 从 flow-local support 逐步上移到
   `tests/harness`，前提是至少两个 flow 需要同一能力。
3. 优先处理仍在 API wallet domain 内的漏网接口，例如
   `query_wallet_activation_info`。
4. 再按 flow 处理 withdraw、collect、fee、resource operation、resource
   reclaim worker。
5. 对真实环境联通性测试，统一放到 `smoke/live`，默认测试和 CI 主链不运行。

## Review Checklist

迁移或新增 backend 相关测试时，review 先看这些问题：

- 测试入口是否可能触发真实 backend。
- 被测 flow 的所有 backend 接口是否有 fake 或明确 smoke 标记。
- fake 是否记录了调用。
- 复杂 flow 是否使用接口级 fake 和脚本队列，而不是 case-specific fake。
- 未配置调用是否可见。
- `then` 是否断言了调用次数和关键参数。
- 失败路径是否断言没有多余副作用。
- 测试正文是否仍然是业务剧本，而不是 backend mock 细节堆叠。
