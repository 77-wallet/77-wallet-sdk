# Ordinary EVM transfer address gate

2026-10-20, user authorizes same-process concurrent nonce protection only. Broadcast-before-bill and uncertain-send persistence are explicitly deferred.

| Contract | Evidence |
| --- | --- |
| Ordinary EVM entry waits behind the API sender gate before any preflight/RPC/signing | ordinary_evm_transfer_waits_for_api_address_gate_before_preflight: hold API permit, adapter future remains pending; release then invalid fee fails without RPC, next permit succeeds |
| Same EVM chain/address spelling aliases share one permit | transfer_gate_evm_aliases_wait_and_resume_after_release |
| Independent addresses/chains remain parallel; non-EVM case semantics unchanged | transfer_gate_independent_addresses_and_chains_do_not_block |
| Cancelled waiter and aborted owner do not poison the gate | transfer_gate_cancelled_waiter_does_not_poison_slot, transfer_gate_aborted_owner_releases_slot |
| Existing native/token amount, fee and broadcast behavior remains | existing evm_fee::flow_tests, local fake RPC/public test-only signing key |

No API nonce counter/transaction table is introduced into ordinary transfers. Owned permit scope covers the full Ethereum adapter branch. No cross-process/restart or RPC freshness guarantee is inferred from this gate.
