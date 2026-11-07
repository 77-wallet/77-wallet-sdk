# EVM Transfer Fee Module Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the Scroll-specific standard-wallet fee code into an extensible EVM fee module without changing the existing quote contract.

**Architecture:** `TransactionAdapter` delegates its Ethereum fee arm to `evm_fee::estimate_transfer_fee`. That module owns shared balance, backend tracker, execution gas, and response logic; one policy match chooses chain-specific additional fees. `evm_fee/scroll.rs` owns the Scroll RPC call.

**Tech Stack:** Rust 2024, Tokio, Alloy `TransactionRequest`/`U256`, existing `wallet-transport` RPC client.

**Spec:** `docs/superpowers/specs/2026-09-28-evm-transfer-fee-design.md`

## Global Constraints

- Work in `codex/feat-2.2-evm-chains-sdk`; do not commit, merge, or push before user review.
- Preserve front-end parameters, backend gas-tracker protocol, existing three-tier JSON, and non-Scroll behavior.
- Only Scroll gets L1 data fee now. Polygon/Avalanche retain execution-only fees; Arbitrum/Base retain existing behavior pending separate verification; Fantom is excluded.
- Default tests stay offline. Do not expose RPC auth or production configuration.

## Review Focus

1. Non-Scroll EVM quotes must not call Scroll RPC; Task 2 tests policy selection.
2. Scroll ERC-20 RPC payload must contain token contract and transfer calldata; Task 1 tests the request.
3. RPC errors, malformed values, and zero must fail rather than yield an understated quote; Task 1 tests each case.
4. L1 fee increases all three total estimates and maxima, not signed `feeSetting`; Task 3 tests the response.
5. Internal-node method support remains unverified without a separate live probe; Task 3 reports this explicitly.

---

### Task 1: Isolate Scroll RPC

**Files:** Create `wallet-api/src/domain/chain/adapter/evm_fee/mod.rs`, `scroll.rs`, and `scroll_tests.rs`; register `evm_fee` in `wallet-api/src/domain/chain/adapter/mod.rs`. Keep the current `scroll_fee.rs` active until Task 2 switches the caller, then remove it.

**Interface:** `pub(super) async fn estimate_l1_data_fee(rpc_url: &str, headers: Option<HashMap<String, String>>, tx: TransactionRequest) -> Result<U256, ServiceError>`.

- [x] Create `evm_fee/mod.rs` registering `scroll` and move native/ERC-20/invalid/zero tests into `scroll_tests.rs`; add a JSON-RPC error test using the local fake server.
- [x] Move the existing RPC implementation into `scroll.rs`, preserving timeout, auth headers, wei parsing, and explicit errors. Register `evm_fee` in the parent adapter module and run `cargo test -p wallet-api --lib --no-default-features scroll_l1_fee`; verify the migrated tests pass before changing the caller.

### Task 2: Extract EVM quote orchestration

**Files:** Extend `wallet-api/src/domain/chain/adapter/evm_fee/mod.rs`; create `tests.rs`; modify `wallet-api/src/domain/chain/adapter/transaction_adapter.rs`; remove old `scroll_fee.rs`.

**Interface:** `pub(super) async fn estimate_transfer_fee(ctx: &'static Context, chain: &EthChain, req: &BaseTransferReq, backend_api: &BackendApi, currency: &str, token_currency: TokenCurrency) -> Result<String, ServiceError>`. The caller resolves `main_symbol` into `token_currency` before delegation; passing it again would be redundant.

- [x] Add a policy test: Scroll selects its additional-fee RPC; Ethereum/BNB/Polygon/Avalanche/Arbitrum/Base do not in this batch. Confirm it fails before adding the policy.
- [x] Move the existing Ethereum-arm balance check, tracker, `TransferOpt`, `estimate_gas`, and response conversion into `evm_fee`; select extra-fee behavior in one `match`.
- [x] Replace the Ethereum arm with one module call; remove the Scroll condition and unused imports. Run focused EVM fee tests.

### Task 3: Generic response helper and verification

**Files:** Modify `wallet-api/src/response_vo/standard_wallet/transaction.rs` and `docs/codex/assertion-matrices/standard-wallet-transfer-fee.md`.

- [x] Rename `with_l1_data_fee` to `with_additional_native_fee`; preserve arithmetic and JSON fields. Extend the existing test to assert all three tiers.
- [x] Run `cargo test -p wallet-api --lib --no-default-features scroll_l1_fee`, `cargo check -p wallet-api --lib --no-default-features`, `cargo test -p wallet-api --lib --no-default-features`, `cargo fmt --all -- --check`, and `git diff --check`.
- [x] Review `git status --short` and the final diff. Report the live-node verification gap and stop uncommitted for user review.
