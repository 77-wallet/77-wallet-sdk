# Five EVM chains Safe multisig plan

## Scope and verified premise

Enable Safe multisig on Polygon PoS, Avalanche C-Chain, Arbitrum One, Base,
and Scroll. Fantom is excluded. This is a separate flow from swap and the
ordinary EVM transfer/fee work.

The SDK's pinned `wallet_core` revision has no `ChainCode` variants for these
five chains. A later Core commit (`9785629d190bdf6ca62e1d0a3695c5f33dba3fbd`,
on remote `codex/feat-2.2-evm-chains-core`) already adds the new EVM chain
codes; reuse it rather than reimplementing chain recognition. Its Safe 1.3.0
deployment code uses the canonical ProxyFactory
`0xa6B71E26C5e0845f74c812102Ca7114b6a896AB2` and SafeL2 singleton
`0x3E5c63644E683549055b9Be8653de26E0B4CD36E`. Safe's official v1.3.0
deployment registry lists both canonical contracts on all five mainnet chain
IDs: 137, 43114, 42161, 8453, and 534352. Before enabling each chain,
verify `eth_chainId` and `eth_getCode` with the product's configured RPC and
compare code hashes with the registry. Registry entries are not an RPC test.

## Batches

1. **Core prerequisite: satisfied by existing Core revision.** Confirm that
   the new chain codes are in a remote Core revision descended from the SDK's
   pinned revision. The five canonical Safe contracts are registered by Safe;
   verify chain ID and bytecode against the product RPC during per-chain smoke
   tests. Do not duplicate Core chain-recognition changes.
2. **SDK Polygon account flow.** Pin the Core revision, route Polygon through
   the EVM multisig adapter, and cover account creation, deployment, and fee
   estimation. Add offline success and failure integration tests and update
   the multisig assertion matrix. Validate focused wallet-api multisig tests,
   `cargo check -p wallet-api`, formatting, and `git diff --check`. **Stop**
   before transfer/sign/execute or enabling other chains.
3. **SDK Polygon transaction flow.** Cover native and ERC-20 proposal,
   signatures, execution, failed execution, and queue recovery. Extend the
   existing ETH/BNB queue-chain guard. Validate focused multisig transaction
   integration tests and `cargo check -p wallet-api`. **Stop** before enabling
   the remaining chains.
4. **Other chains, one at a time.** Verify configured RPC chain ID, both Safe
   contracts' code hashes, deployment and execution fees, then run create,
   deploy, sign, execute, and recovery checks for Avalanche, Arbitrum, Base,
   and Scroll. Validate the smallest affected tests and a manual smoke run
   per chain. **Stop** on any contract, fee, or receipt mismatch.

Each batch is limited to one module and one flow. No batch may silently enable
the next chain. Follow `docs/codex/testing.md` and the PR definition of done.

## Current round stop condition

Create an SDK branch based on `develop` and record this plan in `PLANS.md`.
Pin the existing Core revision, close exhaustive-match compilation failures
without enabling unrelated business flows, and prove Polygon chooses the EVM
multisig adapter with an offline red/green test. Stop before account deployment,
transaction execution, or enabling the other chains. The next batch starts at
Polygon account creation/deployment and must verify configured RPC contracts.
