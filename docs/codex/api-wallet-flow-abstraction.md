# API Wallet Flow Abstraction

<!-- markdownlint-disable MD013 -->

## Purpose

归集、提币、手续费、资源委托/回收目前都承担了类似的编排职责：

- 接收或创建一条业务任务。
- 向后端发送任务 ACK。
- 判断是否允许进入链上动作。
- 构建交易、广播交易、恢复链上结果。
- 上传执行回执。
- 向后端发送结果 ACK。
- 在失败、重试、恢复、重复消息和进程重启时保持幂等。

这些流程的业务目标不同，但生命周期骨架高度相似。后续新增流程时，不应再复制一套
scanner、dispatcher、worker、stage predicate 和 side effect 规则，而应基于一套共享的
生命周期契约扩展流程能力。

本文档先约束抽象边界，不在本批次修改运行时代码。

## Current Flow Map

### Collect

Current location:

- `wallet-api/src/infrastructure/api_trans/collect/`
- Shadow stages:
  `NeedOrderAck -> NeedResourceGate -> CanBuild -> NeedTxFeeResAck -> CanBroadcast -> NeedRecover -> NeedTxExecReceiptUpload -> NeedResultAck -> NeedServiceFeeUpload`
- Main entity: `api_collect`

Flow-owned behavior:

- 归集订单通知和 order ACK。
- TRON 资源 gate，包含平台/本地资源委托、账户激活依赖、失败 bypass。
- 归集交易构建时的服务费短缺判断。
- 服务费上传后等待 `TX_FEE_RES` ACK 再继续当前交易周期。
- 归集完成后的本地资源回收触发。

Shared behavior candidates:

- stage order definition and `next_expected_fact` metadata.
- build-slot claim/release/reclaim rules.
- broadcast/recover/uncertain-rebroadcast lifecycle.
- terminal fact to receipt upload to result ACK progression.
- diagnosis and stuck-monitor severity/wait threshold metadata.

### Withdraw

Current location:

- `wallet-api/src/infrastructure/api_trans/withdraw/`
- Shadow stages:
  `NeedTxAck -> NeedResourceGate -> CanBuild -> CanBroadcast -> NeedRecover -> NeedTxExecReceiptUpload -> NeedTxResAck`
- Main entity: `api_withdraw`

Flow-owned behavior:

- 提币 TX ACK and `WD` backend payload.
- TRON resource gate release and failed-delegation bypass.
- 提币审核、订单状态、手续费展示、实际资源展示。
- Withdraw-specific scanner wakeups after resource projection.

Shared behavior candidates:

- task ACK/result ACK idempotency.
- resource-gate dependency model.
- build/broadcast/recover/receipt progression.
- chain terminal and pre-broadcast terminal classification.
- duplicate command suppression and watchdog timing.

### Collect Fee

Current location:

- `wallet-api/src/infrastructure/api_trans/collect_fee/`
- Shadow stages:
  `NeedTxAck -> CanBuild -> CanBroadcast -> NeedRecover -> NeedTxExecReceiptUpload -> NeedTxResAck`
- Main entity: `api_fee`

Flow-owned behavior:

- 服务费订单 payload and ACK type.
- Parent collect order dependency: fee success or failure decides whether the
  origin collect can continue, retry, or fail.
- Fee-specific uncertain rebuild and stale build-slot repair.

Shared behavior candidates:

- simple transaction lifecycle without resource gate.
- task ACK, build, broadcast, recover, receipt upload, result ACK.
- final result projection back to an origin order through a typed dependency.

### Resource Delegation And Reclaim

Current location:

- `wallet-api/src/infrastructure/api_trans/resource_delegate/`
- `wallet-api/src/infrastructure/api_trans/resource_reclaim/`
- `wallet-api/src/infrastructure/api_trans/resource_delegation.rs`
- Main entity: `api_resource_delegation`

Flow-owned behavior:

- Resource operation type: `Delegate` vs `Undelegate`.
- Source: `Platform` vs `Local`.
- Origin type: collect or withdraw.
- Projection rule that releases or keeps the origin resource gate.
- Capacity reservation for concurrent delegate operations.
- Pre-broadcast permanent failure facts versus post-broadcast chain failure
  facts.

Shared behavior candidates already emerging:

- `resource_delegate/local_execution.rs` centralizes local delegate
  claim/broadcast/recovery.
- `resource_chain_facts.rs` centralizes success/failure/receipt eligibility.
- `resource_gate_projection.rs` centralizes projection into origin gates.

### Resource Operation

Current location:

- `wallet-api/src/infrastructure/api_trans/resource_operation/shadow.rs`
- Main entity: `api_resource_operation`

Flow-owned behavior:

- Independent platform resource stake/unstake style operations.
- Backend task source restriction.
- Resource operation detail display.

Shared behavior candidates:

- lifecycle intents:
  `SendTaskAck -> ClaimBuildSlot -> BroadcastTx -> RecoverTx -> UploadTxExecReceipt -> SendResultAck`
- build-slot retry recovery.
- broadcast-only rows must not become success before chain/result confirmation.

## Shared Lifecycle Vocabulary

Every flow should be describable as a sequence of facts and side effects, not as
an ad hoc worker script.

### Durable Facts

Durable facts are persisted state transitions. They are the only source of truth
after process restart.

Common durable facts:

- `task_ack_sent_at` / `tx_ack_sent_at` / `order_ack_sent_at`
- `resource_gate_released_at`
- `building_at`
- `raw_tx`
- `tx_hash`
- `last_broadcast_at`
- `broadcast_uncertain_since_at`
- `transaction_time`
- `chain_success_at`
- `chain_failed_at`
- `err_code`
- `tx_exec_receipt_uploaded_at`
- `result_ack_sent_at` / `tx_res_ack_sent_at`
- `finished_at`

Rule: scanner and dispatcher may propose work from facts; workers may create
facts; side effects must be claimed or made idempotent before external calls.

### Terminal Facts

The transaction module already documents the terminal model in
`wallet-api/src/infrastructure/api_trans/mod.rs`. The abstraction should keep
that model authoritative:

- On-chain terminal: `transaction_time IS NOT NULL`.
- Build-stage business failure: `build_blocked_at IS NOT NULL` with
  `need_service_fee = true`.
- Optional explicit termination: `finished_at IS NOT NULL` without chain/build
  terminal facts, only when a concrete reason is persisted.

Resource delegation extends the same idea:

- Pre-broadcast permanent failure is terminal, but must not write a false
  `chain_failed_at`.
- Post-broadcast recovered failure is a chain terminal and must persist the
  matching broadcast hash and failure facts atomically.

Rule: terminal facts are exclusive. Once a terminal exists, later work may only
complete allowed notification/ACK side effects.

### Side Effects

Side effects are external actions or local projections caused by durable facts.

Common side effects:

- Send task/TX/order ACK to backend.
- Query chain resource/account state.
- Build raw transaction.
- Broadcast raw transaction.
- Recover chain result by transaction hash.
- Upload execution receipt.
- Send result ACK to backend.
- Project resource result back to origin collect/withdraw gate.
- Wake scanner or enqueue a follow-up command.

Rule: every side effect needs a durable idempotency fact, a retry policy, and a
clear owner. Shared code may execute the side effect, but flow-owned code must
own payload shape and business projection.

## Proposed Abstraction

### FlowSpec

Introduce a flow description trait in a later implementation batch. It should
describe metadata and ordering, not hide business behavior behind a large
generic service.

Candidate responsibilities:

- Flow name and trade number field.
- Entity type and repository adapter.
- Stage list in scanner order.
- Stage predicate provider for SQL and Rust predicates.
- External payload builders for ACK and receipt calls.
- Build transaction adapter.
- Broadcast/recover adapter.
- Terminal classifier.
- Flow-owned projection hooks.

The trait should be small enough that collect, withdraw, fee, and resource
operation can implement it without losing readability.

### FlowStage

Use one shared stage vocabulary with optional capabilities:

| Shared stage | Meaning | Current examples |
| --- | --- | --- |
| `NeedTaskAck` | backend/order/task ACK not durably sent | collect order ACK, withdraw TX ACK, fee TX ACK, resource task ACK |
| `NeedResourceGate` | chain action is blocked by resource dependency | collect and withdraw TRON token flows |
| `CanBuild` | all preconditions met and no raw transaction fact exists | collect, withdraw, fee, resource operation |
| `NeedDependencyAck` | child side effect must be ACKed before parent continues | collect fee result ACK |
| `CanBroadcast` | signed raw transaction exists without broadcast fact | all transaction-like flows |
| `NeedRecover` | broadcast hash exists and terminal chain result is missing | all transaction-like flows |
| `NeedReceiptUpload` | terminal result exists and execution receipt is missing | all transaction-like flows |
| `NeedResultAck` | backend result ACK is missing | withdraw, fee, collect, resource |
| `NeedFollowup` | terminal side effect spawns cleanup or dependent work | collect local undelegation |
| `Blocked` | no safe progress point exists | diagnose only |

Local enum names may stay during migration, but each local stage should map to
one shared stage and declare flow-owned payload/transition details.

### FlowRepository

Each flow table has different column names and status enums. Avoid forcing a
single table model. Instead define repository adapters that expose common
capabilities:

- scan records for a stage.
- load by trade number.
- claim and release build slot.
- persist build fact: raw transaction plus deterministic hash.
- persist broadcast fact.
- persist uncertain broadcast fact and optional rebuild invalidation.
- persist chain success or chain failure atomically.
- claim receipt upload.
- mark receipt upload.
- claim result ACK.
- mark result ACK.
- mark finished where the terminal contract allows it.

Adapters can delegate to existing repositories. The first extraction should wrap
existing methods, not move database logic across crates.

### FlowWorker

Shared worker code should handle the repeatable shell:

1. Load record.
2. Check terminal guard.
3. Claim idempotency slot if needed.
4. Execute one side effect.
5. Persist the durable fact before returning success.
6. Return a typed outcome for the flow-owned projection hook.

Flow-owned hooks should stay explicit:

- `on_resource_gate_released`
- `on_build_blocked`
- `on_receipt_uploaded`
- `on_result_ack_sent`
- `on_child_flow_terminal`
- `on_terminal_followup`

This keeps collect-specific fee/resource behavior from leaking into withdraw,
and keeps withdraw-specific ACK/display rules out of collect.

## Rules For Future Flow Additions

Before adding a new API wallet flow:

1. Define the flow's durable facts and terminal facts.
2. Map every intended scanner point to `FlowStage`.
3. Identify all external side effects and their idempotency facts.
4. Decide whether the flow has a resource/dependency gate.
5. Define how child-flow terminal results project back to the origin.
6. Add a focused assertion matrix entry before implementation.
7. Add tests for at least one happy path and one failure/no-side-effect path.

The flow must not introduce:

- a new private retry model when a shared retry policy applies.
- a scanner predicate that cannot be expressed as a durable fact query.
- a side effect without a persisted claim or completion fact.
- a terminal transition that can coexist with another terminal transition.
- a hidden backend or chain dependency in default tests.

## Migration Plan

### Batch 1: Documentation And Vocabulary

Scope:

- This document.
- Existing assertion matrices remain source-specific.
- No runtime behavior change.

Validation:

```sh
npx markdownlint-cli2 docs/codex/api-wallet-flow-abstraction.md PLANS.md
git diff --check
```

Stop condition:

- Team can review the lifecycle vocabulary and decide the first implementation
  target.

### Batch 2: Stage Metadata Unification

Scope:

- Extract shared stage metadata: name, severity, wait threshold, expected fact.
- Keep existing enum variants and predicates in each flow.
- Add unit tests proving collect/withdraw/fee metadata matches current values.

Validation:

```sh
cargo test -p wallet-api --lib --no-default-features --features api-mqtt,dev stage -- --nocapture
cargo fmt --check
git diff --check
```

Stop condition:

- Diagnose/stuck-monitor metadata no longer needs to be copied across flows.

### Batch 3: Build/Broadcast/Recover Core

Scope:

- Start with collect-fee or resource-operation because they have fewer
  flow-owned gates.
- Extract build-slot, broadcast fact, uncertain recovery, receipt upload, and
  result ACK shell behind repository adapters.
- Keep payload builders and terminal projections flow-owned.

Validation:

```sh
cargo test -p wallet-api --lib --no-default-features --features api-mqtt,dev build_slot -- --nocapture
cargo test -p wallet-api --lib --no-default-features --features api-mqtt,dev confirmation_status -- --nocapture
cargo fmt --check
git diff --check
```

Stop condition:

- One low-dependency flow uses the shared execution shell with no behavior
  change.

### Batch 4: Resource Gate Contract

Scope:

- Make resource gate a shared dependency stage with flow-owned projections.
- Reuse existing `resource_gate_projection` and `local_execution` boundaries.
- Keep collect and withdraw origin release rules separate.

Validation:

```sh
cargo test -p wallet-api --lib --no-default-features --features api-mqtt,dev resource_gate -- --nocapture
cargo test -p wallet-api --lib --no-default-features --features api-mqtt,dev local_delegate_execution -- --nocapture
cargo fmt --check
git diff --check
```

Stop condition:

- Collect and withdraw share the gate lifecycle contract while still owning
  different release/bypass semantics.

### Batch 5: New Flow Template

Scope:

- Add a documented template for future API wallet flows.
- Include required repository adapter methods, stage mapping, side-effect
  claims, and assertion matrix checklist.

Validation:

```sh
npx markdownlint-cli2 docs/codex/api-wallet-flow-abstraction.md docs/codex/assertion-matrices/README.md
git diff --check
```

Stop condition:

- New flows can start from a lifecycle checklist rather than copying collect or
  withdraw workers.

## Acceptance Criteria

- Existing flow behavior remains unchanged until an implementation batch
  explicitly changes it.
- Every shared abstraction is introduced behind current repository/service
  boundaries.
- Each extraction batch updates the affected assertion matrix.
- Each extraction batch has focused tests for success, retry/recovery, and
  failure/no-side-effect behavior.
- The implementation never collapses collect, withdraw, fee, and resource
  business projections into one opaque generic handler.
