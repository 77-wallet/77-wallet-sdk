# CLAUDE.md

This file provides guidance for AI coding agents working in this repository.

## 中文版

### 1. 快速开发命令

```bash
# 编译与检查
cargo check
cargo build

# 测试（按需缩小范围）
cargo test -p wallet-api
cargo test -p wallet-transport-backend
cargo test -p wallet-database

# 全量检查
cargo fmt --all
cargo clippy --all-targets --all-features
```

说明：
- 优先先跑 `cargo check`，再按改动范围跑定向测试。
- 大改动（跨模块、状态机、数据库迁移）建议补跑全 workspace 测试。

### 2. 代码分层与调用关系
- 入口编排：`wallet-api/src/manager.rs`（`WalletManager`）
- API 包装层：`wallet-api/src/api/*`
- Service 业务层：`wallet-api/src/service/*`
- Domain 领域层：`wallet-api/src/domain/*`
- 数据层：`wallet-database` 的 `entities + repositories + migrations`
- 后端通信：`wallet-transport-backend`

典型链路：
1. `api/*` 接收参数并调用 service。
2. `service/*` 编排业务流程（鉴权、签名、状态推进、落库）。
3. `domain/*` 承担核心规则和链适配逻辑。
4. 通过 `wallet-transport-backend` 与后端 API 协同。
5. 通过 `wallet-chain-interact` 与链节点交互。

### 3. 当前仓库核心业务域
- 普通交易：转账、手续费估算、交易结果同步。
- 多签账户：创建、成员确认、服务费、部署、状态恢复。
- 多签交易队列：创建、签名、执行、取消、状态同步。
- 权限交易：TRON permission 修改、授权签名路径。
- 质押/委派/投票：TRON freeze/unfreeze/delegate/vote/reward。
- Swap：报价、授权、兑换、授权记录取消。
- API 钱包：充值/提币/归集/手续费单、策略管理、地址扩展。

### 4. 关键状态模型（必须保持一致）
- 多签账户状态：
  - `Pending(1)` -> `Confirmed(2)` -> `OnChain(3)`
  - 失败态：`OnChainFail(4)`
  - 上链中：`OnChianPending(5)`
- 多签账户支付状态：
  - `Unpaid(0)` / `Paid(1)` / `PaidFail(2)` / `PaidPending(3)`
- 多签队列状态：
  - `PendingSignature(0)` -> `HasSignature(1)` -> `PendingExecution(2)` -> `InConfirmation(3)` -> `Success(4)`/`Fail(5)`
- 账单状态：`Pending(1)` / `Success(2)` / `Failed(3)`
- API 资金单（collect/fee/withdraw）采用“事实驱动字段”为主，`status` 主要用于展示与兼容。

### 5. 修改时的强约束
- 不要直接破坏状态推进顺序。
- 不要只改 `status` 而忽略事实时间字段（`*_sent_at`, `*_uploaded_at`, `finished_at` 等）。
- 广播不确定态（uncertain）相关字段必须成组维护，避免出现“已完成但未回收”的不一致记录。
- 涉及 nonce 的逻辑要确保可恢复（尤其 EVM 交易）。

### 6. 注释与日志
- 注释写业务意图和约束来源，不重复代码语义。
- 日志可以打流程阶段（build/broadcast/ack/reconcile），但禁止打印敏感信息（私钥、助记词、密码）。

### 7. 测试策略
- 功能改动至少验证：
  - 成功路径
  - 失败路径
  - 恢复/重试路径
- 优先补在已有测试目录：
  - `wallet-api/tests/transactions/*`
  - `wallet-api/tests/multisig_*/*`
  - `wallet-api/tests/stake/*`
  - `wallet-transport-backend/tests/*`

### 8. 数据库迁移与兼容性
- 迁移文件在：
  - `wallet-database/schema/migrations`
  - `wallet-database/schema/api_wallet/migrations`
  - `wallet-database/schema/api_funds/migrations`
- 新增字段后：
  - 更新实体结构体
  - 更新 repo 查询/写入
  - 补齐默认值与回填策略（若必要）

---

## English Version

### 1. Quick Commands

```bash
# Build and check
cargo check
cargo build

# Targeted tests
cargo test -p wallet-api
cargo test -p wallet-transport-backend
cargo test -p wallet-database

# Full quality gates
cargo fmt --all
cargo clippy --all-targets --all-features
```

Notes:
- Start with `cargo check`, then run targeted tests for changed modules.
- For cross-module/state-machine/migration changes, run broader tests.

### 2. Layering and Call Flow
- Orchestration entry: `wallet-api/src/manager.rs` (`WalletManager`)
- API wrapper layer: `wallet-api/src/api/*`
- Service layer: `wallet-api/src/service/*`
- Domain layer: `wallet-api/src/domain/*`
- Data layer: `wallet-database` (`entities + repositories + migrations`)
- Backend communication: `wallet-transport-backend`

Typical flow:
1. `api/*` receives request and delegates to service.
2. `service/*` orchestrates business flow (auth/signing/state transition/persistence).
3. `domain/*` implements core rules and chain-adapter logic.
4. Backend coordination through `wallet-transport-backend`.
5. On-chain interaction through `wallet-chain-interact`.

### 3. Core Business Domains in This Repo
- Standard transfers: transfer, fee estimation, tx result sync.
- Multisig account lifecycle: create, member confirmation, service fee, deploy, recovery.
- Multisig queue lifecycle: create, sign, execute, cancel, status synchronization.
- Permission updates: TRON permission update and signer-based flow.
- Stake/delegate/vote: TRON freeze/unfreeze/delegate/vote/reward claim.
- Swap: quote, approve, swap execution, approval cancellation/listing.
- API wallet: recharge/withdraw/collect/fee orders, strategy management, address expansion.

### 4. Critical State Models (keep consistent)
- Multisig account status:
  - `Pending(1)` -> `Confirmed(2)` -> `OnChain(3)`
  - Failure: `OnChainFail(4)`
  - In-progress: `OnChianPending(5)`
- Multisig pay status:
  - `Unpaid(0)` / `Paid(1)` / `PaidFail(2)` / `PaidPending(3)`
- Multisig queue status:
  - `PendingSignature(0)` -> `HasSignature(1)` -> `PendingExecution(2)` -> `InConfirmation(3)` -> `Success(4)`/`Fail(5)`
- Bill status: `Pending(1)` / `Success(2)` / `Failed(3)`
- API funds entities (collect/fee/withdraw) are fact-driven; `status` is mostly narrative/compat.

### 5. Non-negotiable Constraints During Changes
- Do not break state transition ordering.
- Do not update only `status` while ignoring fact timestamps (`*_sent_at`, `*_uploaded_at`, `finished_at`, etc.).
- Uncertain-broadcast fields must be maintained as a coherent set.
- Nonce logic (especially EVM) must remain recoverable.

### 6. Comments and Logging
- Comments should explain intent and constraints, not obvious code behavior.
- Logs may include process phase markers (build/broadcast/ack/reconcile), but never expose secrets.

### 7. Testing Strategy
- At minimum validate:
  - happy path
  - failure path
  - retry/recovery path
- Prefer existing test directories:
  - `wallet-api/tests/transactions/*`
  - `wallet-api/tests/multisig_*/*`
  - `wallet-api/tests/stake/*`
  - `wallet-transport-backend/tests/*`

### 8. Migration and Compatibility
- Migration roots:
  - `wallet-database/schema/migrations`
  - `wallet-database/schema/api_wallet/migrations`
  - `wallet-database/schema/api_funds/migrations`
- After adding fields:
  - update entity structs
  - update repository read/write paths
  - add defaults/backfill strategy when needed
