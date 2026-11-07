# Standard Wallet Transfer Fee Assertions

## EVM additional fee: Core protocol and SDK quote (2.2)

| Case | Test entrypoint | Invariant |
| --- | --- | --- |
| Scroll native transfer L1 quote | Core `evm_additional_fee_scroll_native_uses_same_provider_and_auth` | The selected provider sends the transfer object with its auth header and parses wei. |
| Scroll ERC-20 transfer L1 quote | Core `evm_additional_fee_scroll_erc20_contains_transfer_calldata` | The token contract and ABI-encoded transfer calldata are sent, not a plain ETH transfer. |
| Base native and ERC-20 upper bound | Core `evm_additional_fee_base_native_and_erc20_query_oracle_upper_bound` | Both transfer shapes query Base GasPriceOracle and parse wei. |
| Invalid, zero, or RPC-error response | Core `evm_additional_fee_invalid_zero_and_rpc_errors_fail` | Neither chain silently omits or zeroes a failed additional-fee estimate. |
| Chain-specific fee policy | Core `evm_additional_fee_non_l2_chains_do_not_call_rpc` | Ethereum/BNB/Polygon/Avalanche/Arbitrum retain execution-only fees without extra RPC. |
| Base native SDK quote | SDK `base_native_quote_uses_core_fee_without_changing_execution_settings` | The SDK requests Core's Base fee before estimating execution gas; estimated fee includes the raw L1 quote and maximum includes ceil(20%) extra L1 reserve, leaving execution settings unchanged. |
| Base ERC-20 SDK quote | SDK `base_erc20_quote_keeps_transfer_calldata_and_adds_core_fee` | The SDK checks token balance, keeps ERC-20 transfer calldata, and asks the Base oracle for the larger unsigned transaction size. |
| Polygon/Avalanche/Arbitrum SDK quote | SDK `non_l1_data_fee_chains_quote_three_tiers_without_extra_rpc` | Each chain retains three execution-fee tiers and makes no separate L1-data-fee RPC. |
| Scroll native SDK quote | SDK `scroll_native_quote_uses_core_fee_without_changing_execution_settings` | The SDK sends the native transfer shape to Scroll's L1-data-fee RPC and includes the result without changing execution settings. |
| Scroll ERC-20 SDK quote | SDK `scroll_erc20_quote_keeps_transfer_calldata_and_adds_core_fee` | The SDK sends ERC-20 calldata through Core and includes the L1 fee in the returned quote. |
| Scroll RPC failure in SDK quote | SDK `scroll_additional_fee_rpc_error_fails_sdk_quote` | A Core additional-fee error aborts the SDK quote before execution gas estimation. |
| Three-tier total and execution settings | SDK `evm_additional_fee_is_added_to_each_quote_without_changing_execution_fee_setting` | The same additional fee is added to estimated and maximum amounts for each tier, while execution `feeSetting` and existing JSON fields remain unchanged. |
| Base manual maximum L1 price movement | SDK `base_manual_maximum_quote_covers_l1_rise_but_rejects_excess` | With spendAll=false, the returned maximum covers the observed L1 rise and rounds the 20% margin upward to wei; larger rises still fail the existing fee guard, without changing the transfer amount. |
| Base/Scroll send preflight fee | SDK `send_preflight_reserves_additional_fee_for_base_and_scroll` | Before broadcast, the same transfer shape requires execution fee plus Core's additional L1 fee in native balance. |
| Base/Scroll transfer entry rejects missing L1 fee | SDK `send_entry_rejects_base_and_scroll_when_only_l1_fee_is_missing` | The actual transfer entry returns `InsufficientFeeBalance` when native balance covers the EIP-1559 execution cap but not its additional L1 fee; only balance and L1 quote RPCs occur, with no signing or broadcast. |
| Exact wei send balance boundary | SDK `send_preflight_requires_the_full_fee_at_the_wei_boundary` | Exact native balance passes, and one wei less returns `InsufficientFeeBalance` before broadcast. |
| EIP-1559 send cap reserve | SDK `send_preflight_reserves_submitted_eip1559_fee_cap` | ETH and five 2.2 EVM chains reserve `gasLimit × maxFeePerGas` (matching Core's submitted cap), plus any L1 fee; BNB keeps its historical special calculation. |
| Base/Scroll ERC-20 send preflight | SDK `send_preflight_uses_erc20_transfer_shape_for_base_and_scroll` | The Base unsigned-size bound accounts for ERC-20 calldata; Scroll receives the token contract call rather than a native transfer. |
| Other EVM send preflight fee | SDK `send_preflight_keeps_execution_fee_on_non_l2_chain` | Polygon retains execution-only fee and makes no additional-fee RPC. |
| Scroll send preflight failure | SDK `send_preflight_aborts_when_scroll_additional_fee_rpc_fails` | The additional-fee RPC error aborts before any broadcast attempt. |
| Four-chain authenticated live quote | SDK `added_evm_chains_quote_with_dev_tracker_and_real_nodes` (`smoke-live`, ignored) | Polygon, Avalanche, Arbitrum and Base use the dev tracker and authenticated project mainnet RPC to produce three positive quote tiers with the expected chain IDs; Base's maximum includes an L1 fee. No signing or broadcast occurs. An override RPC receives no project token. |

Scope: standard wallet ordinary native/ERC-20 transfer fee estimation and pre-broadcast fee guard. The backend gas tracker remains the source of L2 execution prices. No durable transaction facts or broadcast side effects are changed by the quote path. Live RPC verification is separate from these offline tests.

The Base fee is a conservative upper-bound before signing, not the exact final L1 charge. Core reserves a 256-byte unsigned EIP-1559 envelope plus calldata (256 bytes for native transfers, 324 for ERC-20 transfers) when querying the oracle. Core owns the chain-specific RPC; SDK adds its result to displayed totals while leaving `feeSetting` unchanged. For ordinary-wallet Base quotes, only the maximum includes an additional ceil(L1 / 5) reserve for price movement between quote and send; estimated fees and Scroll behavior are unchanged. API manual Base quotes now reuse the same maximum-only L1 margin; automatic API funding retains its existing one-time total-fee headroom and exact build fee. The send preflight still compares against the fresh raw oracle fee; a rise beyond the reserve can still fail.

Live read-only verification (2026-09-29): the authenticated 77 Scroll mainnet node returned chain ID 534352 and nonzero `scroll_estimateL1DataFee` results for native-transfer and ERC-20-transfer-shaped requests. The authenticated 77 Base mainnet node returned chain ID 8453 and nonzero `GasPriceOracle.getL1FeeUpperBound(uint256)` results for unsigned sizes 160 and 228 bytes. Base Sepolia also returned a nonzero oracle result for 160 bytes. No transaction was signed or broadcast. This verifies node method availability and response shape, not the complete app/backend tracker-to-UI quote flow or final charged fee.

Dev backend tracker probe (2026-09-30): `token/findGasTracker` returned three positive gas-price tiers for `base`, but returned business error `999` for `scroll`. On the authenticated 77 Scroll node, `eth_feeHistory(20, latest, [25, 50, 75])` returned 21 base-fee entries, 20 reward rows of length 3, and 20 gas-used ratios; `eth_getBlockByNumber` and `eth_gasPrice` also returned values. Thus the backend Scroll tracker failure needs backend-side diagnosis; it is not evidence that the node lacks these RPC methods. SDK `ChainTransDomain::gas_oracle` falls back to a node-derived propose tier if the backend tracker errors, so a full three-tier Scroll quote is not established by this probe. No app quote request or transaction broadcast was performed.

SDK live quote probe (2026-09-30): the ignored `smoke-live` test passed against dev tracker and authenticated project mainnet RPCs for Polygon, Avalanche, Arbitrum and Base. It checked the expected chain ID, three positive quote tiers and Base's additional L1 fee. Scroll was excluded because its backend tracker still returned business error `999`. The test is read-only and does not establish a final charged network fee.

The local Scroll Sepolia fallback in `wallet-api/data/config/node.testnet.toml` is stale because Scroll shut down that testnet after 2026-07-30. It does **not** block dev App validation against a returned 77 mainnet node: dev permits both network types, and automatic node binding prefers an enabled backend node over a local fallback. The 2026-09-28 simulator database snapshot already has both Base and Scroll enabled and auto-bound to active `77_Nodes` mainnet backend nodes (`is_local = 0`); confirm the current runtime binding if the database has since changed.


## Sui gRPC construction after Core revision update (2026-10-23)

- Boundary: adapter construction only, before fee/transfer RPC and before any
  durable transaction state or broadcast. Existing lifecycle/recovery is retained.
- Coverage: `sui_grpc_tests` in `domain/chain/adapter/transaction_adapter.rs`.
- Valid endpoint plus ASCII authentication metadata constructs a lazy client
  using the supplied URL without a running node. Invalid endpoint or metadata
  returns an error before a chain operation can start.
- Layer: unit; no context/DB/backend.
- Command: `cargo test --offline -p wallet-api --no-default-features --features
  dev --lib sui_grpc_tests`.
- Limits: constructor checks do not assert full gRPC response decoding, live
  authentication, deadline behavior, transaction simulation or broadcast. Core
  mapping tests supply separate offline evidence; live acceptance remains open.

## WALLET-2935 Polygon recommended priority floor (2026-10-24)

- `polygon_quote_floors_wallet_2935_priority_fee_at_node_minimum`: synthetic base=100 and propose=124.991445339 Gwei gives the incident tip before correction; Polygon now returns 25000000000 wei in public JSON and Core execution settings, with cap150Gwei. Only the original tip comes from the screenshot; no claim about original quote source or rejection stage.
- `polygon_priority_floor_recalculates_low_tier_and_preserves_equal_and_higher_tiers`: below-floor safe tier (zero original tip) is raised with consistent estimate/max/settings; existing25/28Gwei tiers remain byte-for-byte equivalent in serialized structure. Shared `for_chain` (used by API wallet) retains its original quote for the same oracle.
- `polygon_priority_floor_keeps_other_evm_chain_quotes_unchanged`: Ethereum/BNB/Avalanche/Arbitrum/Base keep original tip and fees; Scroll retains its existing wei-rounded cap policy.
- Existing `non_l1_data_fee_chains_quote_three_tiers_without_extra_rpc` checks the real ordinary-wallet Polygon quote path: all returned tiers use25Gwei, estimates/maxima are recomputed, and RPC count/order stay unchanged. Avalanche/Arbitrum retain old expected amounts.
- Boundary: ordinary native/ERC20 transfer recommended quotes via `FeeDetails::for_standard_transfer`; generic conversion, explicit manual send settings, API wallet and other business quote entrypoints remain outside this fix. No additional async/state writes. Offline evidence cannot establish live node acceptance or the original upstream source.
- Validation: `cargo test -p wallet-api --no-default-features --features dev --lib response_vo::standard_wallet::transaction::tests --offline`; same command filtered to `domain::chain::adapter::evm_fee::flow_tests`. RED before production correction: screenshot and low-tier tests fail, four other response tests pass. GREEN results recorded in PLANS and target evidence.

## WALLET-2967 quote freshness (2026-10-30)

Scope: ordinary ETH/Polygon/Avalanche/Arbitrum/Base/Scroll mainnet transfer quotes
and final signing only. API quote helper retains original result/error policy;
BNB special mapping, testnet refresh, non-EVM, Polygon25gwei tip floor and L2
additional-fee calculations remain unchanged.

- `polygon_fee_freshness_replaces_stale_backend_quote_before_confirmation`: local
  backend base1gwei/current node200gwei, refresh produces base200/tip25/cap270;
  no send. RED observed old quote base1 rather than200.
- `fee_freshness_send_rejects_underpriced_cap_before_signing`:107.8 cap/current200
  rejects with3524 before invalid key is parsed, no raw broadcast and no bill.
  RED previously reached signing and returned540 instead of3524.
- `fee_freshness_checks_mainnet_caps_without_changing_user_fees`: all6chains,
  cap99/100/101 against base100; reject only99, settings never increased.
- Snapshot RPC error and actual network timeout fail closed; absent base fee is
  not assumed zero. BNB/testnet guard skips preserve previous behavior.
- Existing flow fixtures preserve original amount/fee/L1/order assertions; their
  historical-call counts exclude the new block snapshot. Dedicated freshness
  fixtures capture EVERY request to assert the new validation and no-send path.
- App `ordinary_fee_freshness_test`: force refresh unchanged inputs, block on
  errors, discard changed-input results, suppress concurrent confirmation, share
  EVM scope and exclude API/BNB/non-EVM; optional quote ID/time forwarded with
  unchanged fee parameters. Quote IDs are diagnostic, not authorization.
- Safe logs: quote input/source/time + normalized prices, stale-source fallback,
  final tiers/quoteId, App confirmation, selected-node block/base at pre-sign,
  nonce and existing send hash/ack. No password/key/raw transaction/provider URL
  or arbitrary upstream error body in newly added diagnostics.
- Limits: no live signing/broadcasting, device/release verification, automatic
  post-broadcast fee increase, or guarantee of inclusion after network fees rise.

Validation: final SDK lib83/83, App18/18 and targeted App analysis clean. Evidence
under target/wallet-2967-fee-{sdk-final,app-final,app-analyze}.log. App additionally
asserts selected tier retained when still offered; SDK log test links quote ID
and timestamp through pre-sign to signed nonce/hash. No device/release test.

## Ordinary EVM base refresh retains tiers (2026-11-04)

User-approved correction to the WALLET-2967 quote refresh policy: valid backend
tiers keep their original priority fees when repriced against the already-read
latest block base. No extra RPC, tolerance or shared-state/cache mechanism.
Backend failure/invalid tiers still use the existing one-tier node fallback.
Pre-sign cap guard, Polygon/zero-tip floors and Base/Scroll L1 rules remain.

- `base_fee_refresh_preserves_tiers_tips_and_l1_reserve_on_one_wei_rise`: real
  ordinary Base quote through local HTTP/RPC, backend base1gwei/node+1wei;
  retains safe/propose/fast and tips (including existing zero-tip floor), checks
  estimated L1 and maximum-only ceil(L1/5) reserve, no broadcast. RED observed
  exactly one returned tier instead of three.
- `polygon_fee_freshness_replaces_stale_backend_quote_before_confirmation`:
  now also requires all three tier types after base1->200gwei refresh, with
  unchanged25gwei floor and270gwei cap.
- `fee_refresh_preserves_only_offered_tiers_and_keeps_fresh_backend_quotes`:
  equal-base quote stays backend-sourced; higher base preserves the original
  single offered tier/tip, no invented safe/fast and exactly two requests.
- `fee_refresh_keeps_node_fallback_for_failed_or_invalid_backend_quotes`:
  backend failure, under-base tier and malformed price all retain one-tier node
  fallback; inspect RPC calls and assert no raw broadcast.
- Reuse existing `fee_freshness_send_*`, cap boundary/snapshot timeout/error
  coverage and unchanged Base/Scroll quote tests for the signing and L1 guards.

Validation: scoped fee-flow33/33, response calculation6/6, pre-sign guards5/5;
logs `target/base-fee-refresh-{green,response,presign}.log`. Build via
`CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_PACKAGE_WALLET_API_DEBUG=0
CARGO_PROFILE_TEST_PACKAGE_WALLET_API_CODEGEN_UNITS=64 cargo test --offline
--target-dir target/api-receipt-refresh-build -p wallet-api --lib
--no-default-features --features dev domain::chain::adapter::evm_fee --no-run`;
run the emitted executable with the three scoped filters above. Local socket
binding requires sandbox escalation; original sandbox PermissionDenied is an
environment failure, separate from the confirmed runtime RED (one vs three).
Fixed-version independent review is tracked in PLANS.md.
No real signing/broadcast, device package or live-backend guarantee.
