# EVM Fee Core Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move Base and Scroll L1 fee RPC estimation into wallet Core while keeping SDK quote behavior and JSON unchanged.

**Architecture:** `EthChain` selects the chain-specific additional-fee capability and uses its existing `Provider`/`RpcClient`. `wallet-api` retains balance checks, backend gas tracker, gas-limit quotation, fiat conversion, and three-tier response composition.

**Tech Stack:** Rust 2024, Tokio, Alloy `TransactionRequest`/`U256`, existing `wallet-transport::RpcClient`.

**Spec:** `docs/superpowers/specs/2026-09-29-evm-fee-core-boundary-design.md`

## Global Constraints

- SDK branch: `codex/feat-2.2-evm-chains-sdk`; retain its uncommitted Base work and do not touch the main SDK checkout's multisig changes.
- Core branch `codex/feat-2.2-evm-chains-core` is already checked out in a worktree with unrelated Safe preflight edits. Create a clean, temporary Core worktree from its current commit on a distinct temporary branch; do not modify, stash, commit, or delete the existing worktree's edits.
- No commits, merges, or pushes before user review. Core's new revision must be reviewed, committed, and reachable before SDK changes the pinned Core `rev`; until then, SDK validation may use a temporary local Cargo patch that must not be committed.
- Front-end request and three-tier JSON, backend `findGasTracker` contract, and `feeSetting` semantics remain unchanged. No Swap, arbitrary contract call, API Wallet, or fee-algorithm redesign.
- Do not change Arbitrum, Polygon, or Avalanche fee behavior. Failed, invalid, or zero Base/Scroll additional-fee responses fail the quote, never silently become zero.
- Two-repository change is split into Core and SDK batches. Update each repository's `PLANS.md` before its product-code batch; read nearest `AGENTS.md` and testing guidance first.

## File Map

| Repository | File | Responsibility |
| --- | --- | --- |
| Core | `wallet-chain-interact/src/eth/additional_fee/mod.rs` | One `ChainCode` policy and `EthChain::estimate_additional_fee` |
| Core | `wallet-chain-interact/src/eth/additional_fee/base.rs` | Base GasPriceOracle `getL1FeeUpperBound(uint256)` encoding and wei parsing |
| Core | `wallet-chain-interact/src/eth/additional_fee/scroll.rs` | Scroll `scroll_estimateL1DataFee` call and wei parsing |
| Core | `wallet-chain-interact/src/eth/additional_fee/tests.rs` | Offline RPC request, error, and policy tests; flow-local fake server |
| Core | `wallet-chain-interact/src/eth/provider.rs`, `eth/mod.rs` | Reuse existing client through a narrow Provider RPC method; register module |
| SDK | `wallet-api/src/domain/chain/adapter/evm_fee/mod.rs` | Delegate additional fee to Core, keep SDK orchestration |
| SDK | `wallet-api/src/domain/chain/adapter/evm_fee/{base.rs,scroll.rs,base_tests.rs,scroll_tests.rs,test_support.rs,tests.rs}` | Remove direct RPC production implementations and relocate/rework tests to appropriate owning crate |
| SDK | `Cargo.toml`, `Cargo.lock` | Pin reviewed Core revision after Core commit only |
| SDK | `docs/codex/assertion-matrices/standard-wallet-transfer-fee.md` | Capture invariant quote contract and Core ownership |

## Review Focus

1. Core chain policy accidentally calls additional-fee RPC on Ethereum/BNB/Polygon/Avalanche/Arbitrum: Task 1 has a no-RPC policy test for each.
2. ERC-20 transaction input loses transfer calldata during migration: Task 1 checks Scroll request data and Base upper-bound size for ERC-20.
3. Node headers or timeout differ from the chosen transaction provider: Task 1 uses one `RpcClient` fake and checks its auth header; SDK no longer reselects nodes in Task 2.
4. RPC returns malformed, zero, or JSON-RPC error: Task 1 asserts error propagation for Base and Scroll; Task 2 asserts the quote fails instead of returning an understated amount.
5. L1 wei is added to `feeSetting` instead of only displayed totals: Task 2 asserts all three estimate/max totals change while `feeSetting` stays byte-for-byte equivalent.

---

### Task 1: Core chain-specific additional-fee capability

**Files:** Core files from the File Map; Core `PLANS.md`. Tests remain offline and close to `wallet-chain-interact`.

**Interfaces:**
- `EthChain::estimate_additional_fee(&self, tx: TransactionRequest) -> crate::Result<Option<U256>>`.
- `Provider::estimate_scroll_l1_data_fee(&self, tx: TransactionRequest) -> crate::Result<U256>` and `Provider::estimate_base_l1_data_fee_upper_bound(&self, tx: TransactionRequest) -> crate::Result<U256>` or equivalently scoped private helpers using its existing client. Neither exposes raw `RpcClient` to SDK.
- `None` only for chains with no separate L1 fee in this scope; Base/Scroll return `Some(wei)` or an error. Preserve existing SDK Base size constant 160 bytes plus input length, Scroll transaction JSON and `latest` parameter, and Base oracle address `0x420000000000000000000000000000000000000f`.

- [ ] Check the new Core worktree is clean and based on the exact tip of `codex/feat-2.2-evm-chains-core`; record both paths and commit ID in `PLANS.md`.
- [ ] Add failing offline tests: policy matrix; native/ERC-20 request shape; auth header; malformed/zero/RPC error for Base and Scroll. Use the existing test fixtures or a flow-local fake server; no live endpoint.
- [ ] Run the narrow new Core tests and confirm a failure due to the missing capability (red), not an unrelated build error.
- [ ] Add the minimum Provider helper and Base/Scroll implementations; register `EthChain` method. Use Core `crate::Error` conversions; keep policy in one place.
- [ ] Re-run narrow tests, then `cargo test -p wallet-chain-interact --lib --no-default-features`, `cargo check -p wallet-chain-interact --lib --no-default-features`, `cargo fmt --all -- --check`, and `git diff --check`; record exit codes and failure counts.
- [ ] Stop with Core diff uncommitted for user review. Do not start the SDK revision bump before Core review and an explicitly approved Core commit.

### Task 2: SDK consumes Core capability

**Prerequisite:** User reviewed Task 1; Core commit is explicitly authorized and reachable to SDK Cargo. Do not infer commit authorization from approval of this plan.

**Files:** SDK files from the File Map; SDK `PLANS.md`.

**Interfaces:** Consume `EthChain::estimate_additional_fee(TransactionRequest) -> crate::Result<Option<U256>>`; existing SDK `ServiceError` converts Core `Error`. The front-end response remains `FeeDetails::to_resp(...)` with `with_additional_native_fee(U256)` only for `Some`.

- [ ] Update SDK `PLANS.md` for this batch, including exact Core revision, narrow validation commands, and stop condition.
- [ ] First add/adjust SDK regression tests: Base/Scroll native and ERC-20 totals include additional fee; all three tiers' `estimateFee`/`maxFee` change; `feeSetting` does not; non-additional chains keep execution-only fees; Core error fails quote.
- [ ] Run the narrow SDK tests as a behavior baseline. Because this is an ownership refactor, existing correct quote assertions may already pass; do not claim a fabricated red. If a new assertion targets a missing behavior, record its actual red result before implementation.
- [ ] Replace policy and node re-selection in `evm_fee/mod.rs` with one Core method call; delete obsolete SDK Base/Scroll RPC modules and relocate their protocol tests to Core. Do not alter balance, tracker, gas-estimation, or JSON field names.
- [ ] Pin the reviewed Core commit in all relevant workspace dependencies and lockfile; verify no remaining reference to obsolete revision for `wallet-chain-interact` and its Core siblings.
- [ ] Run narrow EVM fee tests, `cargo test -p wallet-api --lib --no-default-features`, `cargo check -p wallet-api --lib --no-default-features`, `cargo fmt --all -- --check`, and `git diff --check`; record exit codes and failure counts.
- [ ] Review the SDK diff and assertion matrix; stop uncommitted for user review. Do not merge or push.

## Completion Check

For each batch, inspect the exact diff and `git status --short` before reporting. Compare the resulting behavior against each section of the spec; explicitly report any missing live-node verification. Do not claim successful migration until the fresh test commands above have passed in the actual worktrees.
