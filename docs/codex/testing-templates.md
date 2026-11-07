# Testing Templates

本文件是可复制的测试模板手册。先用 `docs/codex/testing.md` 判断测试层级，
再从本文复制对应模板。

## One-Screen Contract

新增或重写测试时，先满足这几条：

- Unit / Component / Smoke 使用 Arrange-Act-Assert。
- Integration 使用 Given-When-Then。
- 默认测试必须离线，不碰真实 backend、真实链 RPC、固定 `test_data`。
- 真实环境验证只能放到 `tests/smoke/`，并且必须 `#[ignore]`。
- Integration 正文不能暴露 SQLite、JSON、channel、backend 解密等底层噪音。
- 每个关键 flow 至少有成功路径和一个失败不变性断言。
- 涉及 flow 覆盖变化时，同步 `docs/codex/assertion-matrices/`。

## Layer Decision

选择能证明行为的最低层级。

| Layer | 用来证明 | 允许 | 禁止 |
| --- | --- | --- | --- |
| Unit | 一个函数、规则、状态判断 | 小 fixture、纯断言 | 网络、DB、manager、全局 context |
| Component | 一个模块和本地依赖协作 | 临时 SQLite、真实 DAO/repo | 真实 backend、真实链 RPC |
| Integration | 业务 flow 编排 | fake backend/chain、临时数据 | 真实远端、固定 `test_data` |
| Smoke | 真实环境联通性或手工验证 | 真实 backend、真实链 RPC、固定地址 | 默认执行、打印敏感信息 |

不要因为某个 helper 已经存在就升层。能用 Unit 证明的，不写 Integration。

## Read Order

读复杂 integration flow 时按这个顺序：

1. 看目录名：确认业务模块和 flow。
2. 看 `mod.rs`：只读测试名和 Given-When-Then 正文。
3. 看 assertion matrix：确认覆盖了哪些风险。
4. 再看 `support/scenario.rs`：理解每个业务步骤背后的动作。
5. 最后才看 `support/db.rs`、`fixtures.rs`、payload、backend recorder。

如果必须先读 helper 才知道测试在测什么，模板就失败了。

## Directory Contract

```text
<crate>/
  src/...                         # unit / component tests near code
  src/testkit/                    # crate-private test entrypoints only
  tests/
    harness/                      # cross-flow environment and fakes
    integration/<module>/<flow>.rs
    integration/<module>/<flow>/
      mod.rs                      # read-first test cases
      support.rs                  # small facade, if support stays small
      support/
        mod.rs                    # facade, if support needs families
        scenario.rs               # given / when / then public surface
        fixtures.rs               # immutable test inputs
        db.rs                     # SQL / repository setup
        assertions.rs             # low-level assertions
    smoke/<module>/<flow>.rs
```

Use a single `<flow>.rs` only when the whole file stays easy to scan.
Once fixture, SQL, fake backend, payload, or assertions become noisy, split to
`<flow>/mod.rs` plus `support`.

## Naming Contract

Test names use behavior, not implementation:

```text
<flow>_<condition>_<expected_result>
```

Good:

- `withdraw_tx_ack_backend_failure_keeps_fact_unset`
- `collect_receipt_duplicate_upload_is_idempotent`
- `import_bind_backend_reject_does_not_persist_relation`

Avoid:

- `test_success`
- `test_transfer`
- `test_1`
- `mock_test`

Helper names are a small vocabulary. Complex flows should expose role methods;
small flows may use flat prefixed methods.

| Shape | Meaning | Where |
| --- | --- | --- |
| `scenario.given().x` | 准备业务事实或 fake 行为 | role trait |
| `scenario.when().x` | 执行业务入口、worker step | role trait |
| `scenario.then().x` | 断言 DB、backend、通知、副作用 | role trait |
| `given_*` | 小 flow 的平铺 Given 方法 | scenario |
| `when_*` | 小 flow 的平铺 When 方法 | scenario |
| `then_*` | 小 flow 的平铺 Then 方法 | scenario |
| `seed_*` | 插入 DB 事实 | support detail |
| `load_*` | 读取当前状态 | support detail |
| `count_*` | 统计副作用次数 | support detail |
| `assert_*` | 底层字段断言 | support detail |

`given/when/then` 是测试正文语言。`seed/load/count/assert` 是底层工具。

## Integration Template

Integration 的目标是让 `mod.rs` 像业务剧本，而不是像搭环境脚本。

### `mod.rs`

```rust
mod support;

use serial_test::serial;

use support::{FlowFixture, FlowGiven, FlowScenario, FlowThen, FlowWhen, ScenarioRoles};

#[tokio::test]
#[serial]
async fn flow_happy_path_writes_facts_and_notifies_backend() {
    let scenario = FlowScenario::new().await;
    let fixture = FlowFixture::new("happy");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.target_order(&fixture).await;
    given.backend_accepts_target_call(&fixture);

    let result = when.target_step_runs(&fixture).await;

    then.step_succeeds(result);
    then.db_facts_are_complete(&fixture).await;
    then.backend_called_once(&fixture).await;
    then.notification_was_emitted(&fixture);
}

#[tokio::test]
#[serial]
async fn flow_backend_failure_keeps_fact_unset_and_retryable() {
    let scenario = FlowScenario::new().await;
    let fixture = FlowFixture::new("backend_fail");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.target_order(&fixture).await;
    given.backend_rejects_target_call(503, "temporary failure");

    let result = when.target_step_runs(&fixture).await;

    then.step_is_retryable(result);
    then.backend_called_once(&fixture).await;
    then.success_fact_is_not_persisted(&fixture).await;
    then.retry_scanner_can_pick_it_again(&fixture).await;
}
```

`mod.rs` 只允许出现：

- `mod support;`
- 少量 `use`
- 测试用例
- 非常轻的结果断言 helper

不允许出现：

- SQL
- `SqliteContext`
- `serde_json::to_value`
- backend recorder 解密
- channel 创建
- 大段 payload 构造

### `support.rs`

小 flow 可以用一个 `support.rs`：

```rust
use crate::harness::{
    AssertRole, CountRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole,
    ensure_worker_env, next_unique_id, WorkerTestEnv,
};

pub(super) struct FlowFixture {
    pub uid: String,
    pub trade_no: String,
}

impl FlowFixture {
    pub(super) fn new(prefix: &str) -> Self {
        let id = next_unique_id();
        Self {
            uid: format!("uid_{prefix}_{id}"),
            trade_no: format!("T_{prefix}_{id}"),
        }
    }
}

pub(super) struct FlowScenario {
    env: &'static WorkerTestEnv,
}

impl FlowScenario {
    pub(super) async fn new() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();
        Self { env }
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn load(&self) -> LoadRole<'_, Self> {
        LoadRole::new(self)
    }

    fn count(&self) -> CountRole<'_, Self> {
        CountRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(super) trait FlowGiven {
    async fn target_order(&self, fixture: &FlowFixture);

    fn backend_accepts_target_call(&self, fixture: &FlowFixture);

    fn backend_rejects_target_call(&self, status: u16, body: &str);
}

#[async_trait::async_trait(?Send)]
impl FlowGiven for GivenRole<'_, FlowScenario> {
    async fn target_order(&self, fixture: &FlowFixture) {
        self.scenario().seed().target_order(fixture).await;
    }

    fn backend_accepts_target_call(&self, fixture: &FlowFixture) {
        self.scenario()
            .env
            .recorder
            .expect_target_call(&fixture.trade_no);
    }

    fn backend_rejects_target_call(&self, status: u16, body: &str) {
        self.scenario()
            .env
            .recorder
            .fail_next_api_backend_call(status, body);
    }
}

#[async_trait::async_trait(?Send)]
trait FlowSeed {
    async fn target_order(&self, fixture: &FlowFixture);
}

#[async_trait::async_trait(?Send)]
impl FlowSeed for SeedRole<'_, FlowScenario> {
    async fn target_order(&self, fixture: &FlowFixture) {
        seed_target_order(self.scenario().env.db_dir(), fixture).await;
    }
}

#[async_trait::async_trait(?Send)]
pub(super) trait FlowWhen {
    async fn target_step_runs(
        &self,
        fixture: &FlowFixture,
    ) -> Result<(), ServiceError>;
}

#[async_trait::async_trait(?Send)]
impl FlowWhen for WhenRole<'_, FlowScenario> {
    async fn target_step_runs(
        &self,
        fixture: &FlowFixture,
    ) -> Result<(), ServiceError> {
        run_target_worker_step(&fixture.trade_no).await
    }
}

#[async_trait::async_trait(?Send)]
pub(super) trait FlowThen {
    fn step_succeeds(&self, result: Result<(), ServiceError>);

    fn step_is_retryable(&self, result: Result<(), ServiceError>);

    async fn db_facts_are_complete(&self, fixture: &FlowFixture);

    async fn success_fact_is_not_persisted(&self, fixture: &FlowFixture);
}

#[async_trait::async_trait(?Send)]
impl FlowThen for ThenRole<'_, FlowScenario> {
    fn step_succeeds(&self, result: Result<(), ServiceError>) {
        result.expect("target step should succeed");
    }

    fn step_is_retryable(&self, result: Result<(), ServiceError>) {
        result.expect("retryable failure should not crash worker");
    }

    async fn db_facts_are_complete(&self, fixture: &FlowFixture) {
        let saved = self.scenario().load().target_order(&fixture.trade_no).await;
        self.scenario().assert().success_fact_is_set(&saved);
    }

    async fn success_fact_is_not_persisted(&self, fixture: &FlowFixture) {
        let saved = self.scenario().load().target_order(&fixture.trade_no).await;
        self.scenario().assert().success_fact_is_unset(&saved);
    }
}

#[async_trait::async_trait(?Send)]
trait FlowLoad {
    async fn target_order(&self, trade_no: &str) -> TargetOrder;
}

#[async_trait::async_trait(?Send)]
impl FlowLoad for LoadRole<'_, FlowScenario> {
    async fn target_order(&self, trade_no: &str) -> TargetOrder {
        load_target_order(self.scenario().env.db_dir(), trade_no).await
    }
}

trait FlowAssert {
    fn success_fact_is_set(&self, saved: &TargetOrder);

    fn success_fact_is_unset(&self, saved: &TargetOrder);
}

impl FlowAssert for AssertRole<'_, FlowScenario> {
    fn success_fact_is_set(&self, saved: &TargetOrder) {
        assert!(saved.success_fact_at.is_some());
    }

    fn success_fact_is_unset(&self, saved: &TargetOrder) {
        assert!(saved.success_fact_at.is_none());
    }
}
```

If `support.rs` grows past one easy screen, turn it into a facade:

```rust
mod assertions;
mod db;
mod fixtures;
mod scenario;

pub(super) use crate::harness::ScenarioRoles;
pub(super) use fixtures::FlowFixture;
pub(super) use scenario::{FlowGiven, FlowScenario, FlowThen, FlowWhen};
```

### Integration Rules

- One test should have one primary `when` action.
- A second `when` action is allowed only for retry, idempotency, recovery, or
  time.
- `Scenario` owns environment, fake backend, DB pools, and notification capture.
- `GivenRole` / `WhenRole` / `ThenRole` are generic harness containers.
- Flow-local traits own the Given / When / Then business method groups.
- `SeedRole` / `LoadRole` / `AssertRole` may organize support internals.
- `CountRole` is reserved for repeated side-effect counting when needed.
- `Fixture` owns unique immutable input data.
- Reset fake state in `Scenario::new`.
- Assert both DB facts and external calls when the flow has both.
- Keep business assertions visible through `then` method names.
- Move only proven cross-flow capability into `tests/harness`.

## Component Template

Use Component when the behavior needs SQLite/repository/domain collaboration
but not backend, chain RPC, manager, or global context.

```rust
#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    struct TestDb {
        _dir: TempDir,
        pool: TargetDbPool,
    }

    impl TestDb {
        async fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let pool = open_temp_pool(dir.path()).await;
            Self { _dir: dir, pool }
        }

        async fn seed_target(&self, trade_no: &str) {
            seed_target_row(&self.pool, trade_no).await;
        }
    }

    #[tokio::test]
    async fn confirm_success_writes_expected_fact() {
        let db = TestDb::new().await;
        let trade_no = "T_COMPONENT_CASE";
        db.seed_target(trade_no).await;

        let outcome = TargetDomain::confirm(&db.pool, trade_no)
            .await
            .expect("confirm step");

        assert!(outcome.should_notify);
        let saved = load_target_row(&db.pool, trade_no).await;
        assert!(saved.confirmed_at.is_some());
        assert!(saved.failed_at.is_none());
    }
}
```

Component rules:

- Use temp SQLite only.
- Do not call `WalletManager::new`.
- Do not depend on `tests/harness`.
- Do not read or write fixed `test_data`.
- Assert persisted DB fields, not only return values.

## Unit Template

Use Unit for pure rules, mapping, validation, parser behavior, or one
single-step decision.

```rust
#[test]
fn diagnose_withdraw_waits_for_audit_when_tx_ack_sent() {
    let withdraw = WithdrawFixture::new()
        .tx_ack_sent()
        .without_audit_result()
        .build();

    let diagnosis = diagnose_withdraw(&withdraw);

    assert_eq!(diagnosis.stage, AdvancementPoint::CanBuild);
    assert_eq!(diagnosis.next_fact, Some("audit_passed_at"));
}
```

Unit rules:

- No SQLite.
- No network.
- No manager.
- No global context.
- Prefer small fixtures over large object graphs.

## Smoke Template

Use Smoke only for real backend, real chain RPC, fixed addresses, local config,
or operator checks.

```rust
#[tokio::test]
#[ignore = "requires live backend, chain RPC, and local smoke config"]
async fn live_flow_reaches_remote_system() -> anyhow::Result<()> {
    wallet_utils::init_test_log();
    let (manager, _params) = get_manager_with_config("client4.toml").await?;
    manager.init_api_swap().await?;

    let result = manager.live_entrypoint().await;

    tracing::info!("live flow result: {result:?}");
    Ok(())
}
```

Smoke rules:

- Every smoke test must have `#[ignore = "..."]`.
- The ignore reason must say what real dependency is required.
- Do not print private keys, mnemonics, tokens, or production config.
- Local smoke config must be ignored by git.
- Smoke does not replace Unit, Component, or Integration coverage.

## Complex Backend Flow Template

Use this shape when one business flow performs several backend calls, such as
`quote -> build -> broadcast -> ack -> receipt`.

Keep the test body business-readable:

```rust
#[serial]
#[tokio::test]
async fn flow_backend_failure_after_broadcast_keeps_fact_unset_and_retryable() {
    let scenario = ComplexFlowScenario::new().await;
    let fixture = ComplexFlowFixture::new("broadcast_fail");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.pending_order(&fixture).await;
    given.backend_script(&fixture)
        .quote_ok()
        .build_ok()
        .broadcast_fails(503, "temporary backend error");

    let result = when.worker_step_runs(&fixture).await;

    then.step_is_retryable(result);
    then.backend_script_was_followed(&fixture).await;
    then.broadcast_success_fact_is_not_persisted(&fixture).await;
    then.scanner_can_retry(&fixture).await;
}
```

Keep the script builder in flow-local `support`, not in `tests/harness`, until a
second flow needs the same vocabulary:

```rust
pub(super) struct ComplexBackendScript<'a> {
    scenario: &'a ComplexFlowScenario,
    fixture: &'a ComplexFlowFixture,
}

impl ComplexBackendScript<'_> {
    pub(super) fn quote_ok(self) -> Self {
        self.scenario.fake_backend.enqueue_quote_ok(&self.fixture.trade_no);
        self.scenario.fake_backend.expect_call("quote", &self.fixture.trade_no);
        self
    }

    pub(super) fn build_ok(self) -> Self {
        self.scenario.fake_backend.enqueue_build_ok(&self.fixture.trade_no);
        self.scenario.fake_backend.expect_call("build", &self.fixture.trade_no);
        self
    }

    pub(super) fn broadcast_fails(self, status: u16, message: &str) -> Self {
        self.scenario
            .fake_backend
            .enqueue_broadcast_error(&self.fixture.trade_no, status, message);
        self.scenario
            .fake_backend
            .expect_call("broadcast", &self.fixture.trade_no);
        self
    }
}
```

Fake rules:

- Fake the interface boundary, not a single scenario.
- Queue configured responses in the same order the flow should call them.
- Record every backend call and assert the expected calls in `then`.
- Make unconfigured critical calls fail loudly or surface as explicit test
  errors.
- Keep payload decoding, SQL, and recorder details below the scenario facade.

## Helper Ownership

Use the narrowest owner.

| Owner | Put here | Do not put here |
| --- | --- | --- |
| Same test file | one-flow seed/assert helpers | shared environment |
| `<flow>/support` | scenario、fixture、SQL、assertions | other flow logic |
| `tests/harness` | fake backend、temp env、collector | business decisions |
| `src/testkit` | crate-private step entrypoint | environment setup |

`harness` is professional test terminology for the shared test rig. It should
feel boring: environment, fake, recorder, collector, fixture primitives.

`testkit` is code-side access for tests. It exposes internal steps; it does not
own the test environment.

## Coverage Checklist

For each important Integration flow, add coverage in this order:

1. Happy path writes final DB facts and sends expected external calls.
2. Input failure returns/records error and does not send side effects.
3. Backend failure keeps success facts unset and leaves the flow retryable.
4. Chain failure records the correct failed fact and avoids success side effects.
5. Duplicate message or retry is idempotent.
6. Recovery resumes from persisted facts.
7. Concurrent execution sends the critical side effect once.

Do not add all seven in one batch by default. Start with happy path plus the
most likely failure invariant.

## Assertion Matrix

When a flow changes, update or add a file under:

```text
docs/codex/assertion-matrices/
```

Each entry should include:

- Flow
- Test entrypoint
- Layer
- Expected DB facts
- Expected backend calls
- Failure invariant
- Remaining coverage gaps

Keep the matrix close to the tests. It is the map; tests are the proof.

## Anti-Patterns

Do not add tests that only do this:

```rust
let res = manager.some_call().await;
tracing::info!("res: {res:?}");
Ok(())
```

Classify them:

- If they need real backend or fixed local state, move to Smoke.
- If they can be asserted offline, turn them into Unit, Component, or
  Integration tests.

Also avoid:

- `common` dumping ground modules.
- `support.rs` shared across unrelated flows.
- `withdraw` importing helpers from `collect`.
- fixed IDs without uniqueness.
- hidden assertions inside `given` methods.
- business behavior hidden in `tests/harness`.
