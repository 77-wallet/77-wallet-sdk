# Testing Backend Fake Inventory

## Purpose

本文档是 backend fake 覆盖清单。
`docs/codex/testing-backend-boundaries.md` 定规则；本文档定当前状态和迁移顺序。

核心目标：

- 新增集成测试前，先知道被测 flow 会不会触发真实 backend。
- 后续补 fake 时，按 backend 接口边界补，不按单个测试场景补。
- 每轮只迁移一个业务 flow，避免一次性重构整个 backend 调用面。

## Current State

当前仓库里有两类 backend 调用：

- `get_api_wallet_backend()`：
  已经可被 `ApiWalletBackend` trait 和 `FakeApiWalletBackend` 接管。
- `get_global_backend_api()`：
  直接拿 `wallet_transport_backend::api::BackendApi`，当前通用 fake 管不住。

现阶段结论：

- `api_wallet/import_bind` 相关测试已经有清楚的 fake 边界。
- `api_wallet` 其他 flow、worker、service、domain 里仍有大量 global backend 调用。
- 不应该马上“每个 backend 模块全写 fake”。正确顺序是按 flow 迁移接口边界。

## Api-Wallet Backend Trait Coverage

`ApiWalletBackend` 当前覆盖了 `wallet-transport-backend/src/api/api_wallet/wallet.rs`
的一部分接口。

| Backend method | Fake coverage | Main test use |
| --- | --- | --- |
| `keys_uid_check` | yes | import/bind uid status |
| `wallet_bind_appid` | yes | scan/bind relation |
| `init_api_wallet` | yes | api wallet init |
| `old_keys_init` | yes | old key init compatibility |
| `query_uid_bind_info` | yes | bind snapshot/query |
| `query_wallet_activation_info` | yes | activation info query |
| `appid_import` | yes | withdrawal import |
| `appid_import_recharge_wallet` | yes | recharge wallet import |
| `appid_uid_usage` | yes | uid usage check |
| `wallet_unbind_appid` | no | no current trait method |
| `save_wallet_activation_config` | no | still global backend |
| `appid_withdrawal_wallet_change` | no | still global backend |

The fake line is therefore partial, not complete.
Tests may trust manager calls only when the flow stays inside covered trait methods.

## Backend Module Inventory

`wallet-transport-backend` currently exposes these backend modules.
This list is intentionally flow-oriented.
A module should become fakeable when a target test flow needs it.

- `api_wallet/wallet`
  Partial `ApiWalletBackend` coverage exists. Extend by flow first.
- `api_wallet/transaction`
  No shared trait fake. Prioritize for collect/withdraw receipt tests.
- `api_wallet/resource_delegation`
  No shared trait fake. Prioritize when testing resource gates.
- `api_wallet/msg`
  No shared trait fake. Prioritize when testing ACK/retry behavior.
- `api_wallet/audit`
  No shared trait fake. Prioritize when testing audit result upload.
- `api_wallet/strategy`
  No shared trait fake. Keep as smoke until strategy flow is migrated.
- `api_wallet/address`
  No shared trait fake. Migrate with expand-address tests.
- `api_wallet/coin`
  No shared trait fake. Migrate with asset/coin integration tests.
- `api_wallet/chain`
  No shared trait fake. Migrate with chain-list tests.
- `api_wallet/swap`
  No shared trait fake. Low priority unless swap flow is target.
- `wallet/*`
  No shared trait fake. Treat as standard wallet scope, not this pilot.

## Direct Global Backend Hotspots

These areas still call `get_global_backend_api()`.
Treat them as unsafe for standard offline integration tests until they are
migrated or intercepted by a dedicated test fake.

- `wallet-api/src/domain/api_wallet/wallet.rs`
  Some methods use the trait, but activation still uses global backend.
- `wallet-api/src/application/api_wallet_withdraw.rs`
  Withdraw orchestration can bypass fake backend.
- `wallet-api/src/infrastructure/api_trans/withdraw/**`
  Withdraw workers upload receipts and ACKs through global backend.
- `wallet-api/src/infrastructure/api_trans/collect/**`
  Collect workers upload receipts and ACKs through global backend.
- `wallet-api/src/infrastructure/api_trans/collect_fee/**`
  Fee workers upload records and side effects through global backend.
- `wallet-api/src/infrastructure/api_trans/resource_operation/**`
  Resource operation side effects use global backend.
- `wallet-api/src/infrastructure/api_trans/resource_reclaim/**`
  Resource reclaim side effects use global backend.
- `wallet-api/src/service/**`
  Many standard wallet services still use global backend.
- `wallet-api/src/domain/assets/mod.rs`
  Asset query and sync can hit global backend.

## Migration Rules

### 1. Do Not Fake Everything Up Front

Do not create a huge fake with every `BackendApi` method before tests need it.
That creates a second backend implementation without clear behavior ownership.

Prefer this sequence:

1. Pick one business flow.
2. List backend calls triggered by that flow.
3. Add or extend a trait boundary for those calls.
4. Implement the fake with call recording and configured responses.
5. Write the integration test using Given-When-Then.

### 2. One Interface Boundary Per Business Surface

Use trait names that describe the business surface, not the transport module.

Recommended examples:

- `ApiWalletBackend` for import/bind/init wallet behavior.
- `ApiWalletTransactionBackend` for receipt upload, restore, and event ACK.
- `ApiWalletResourceBackend` for resource delegation/order side effects.
- `ApiWalletMessageBackend` for message ACK and resend behavior.

Avoid one large `FakeBackendApi` that mirrors all transport methods at once.

### 3. Fake Contract

Every fake method added for integration tests should support:

- call recording
- configured success response
- configured error response
- optional scoped delay only when the flow tests timeout/race behavior
- no silent broad defaults for unconfigured critical calls

### 4. Test Assertions

Every test that crosses a backend fake boundary should assert:

- the business result
- DB facts
- backend call count
- key backend request fields
- absence of forbidden calls on failure paths

## Recommended Next Flow

Next migration should target one of these, in order:

1. `api_wallet/transaction` receipt or ACK flow
   This supports collect/withdraw/fee tests, but touches more worker code.
2. `resource_delegation`
   This aligns with the newly merged resource work, but may include live-chain behavior,
   so standard integration and smoke paths must stay separate.

The previous smallest option, `query_wallet_activation_info`, is now behind the
`ApiWalletBackend` fake boundary.
