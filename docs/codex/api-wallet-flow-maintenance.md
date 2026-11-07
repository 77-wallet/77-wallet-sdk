# API Wallet Flow Maintenance

<!-- markdownlint-disable MD013 -->

## Purpose

Collect, withdraw, collect-fee, and resource delegation/reclaim are maintained
as transaction-like API wallet flows. They have different business payloads, but
they share one lifecycle problem: durable facts drive scanner decisions,
workers perform idempotent side effects, and recovery must survive retries,
duplicate messages, process restart, and partial backend or chain failure.

Resource changes must use this document as the entry gate. Resource is not a
special-case side module; it is the fourth flow in the same maintenance family
as collect, withdraw, and collect-fee.

## Maintained Flow Family

| Flow | Primary location | Assertion matrix |
| --- | --- | --- |
| Collect | `wallet-api/src/infrastructure/api_trans/collect/` | `docs/codex/assertion-matrices/api-wallet-collect.md` |
| Withdraw | `wallet-api/src/infrastructure/api_trans/withdraw/` | `docs/codex/assertion-matrices/api-wallet-withdraw.md` |
| Collect fee | `wallet-api/src/infrastructure/api_trans/collect_fee/` | `docs/codex/assertion-matrices/api-wallet-collect.md` |
| Resource | `wallet-api/src/infrastructure/api_trans/resource_delegate/`, `resource_reclaim/`, `resource_operation/`, `resource_delegation.rs` | `docs/codex/assertion-matrices/api-wallet-resource.md` |

The longer abstraction direction is documented in
`docs/codex/api-wallet-flow-abstraction.md`. This file is the operational rule:
what must be checked before changing one of these flows today.

## Lifecycle Gate

Before implementing a non-trivial change to any maintained flow, identify which
of these lifecycle stages the change touches:

| Stage | Maintenance question |
| --- | --- |
| Task or TX ACK | Is the backend ACK claimed or persisted before retry can duplicate it? |
| Resource gate | Can a stale or unrelated resource result release the wrong origin order? |
| Build slot | Can retry, skip, or crash leave a permanent in-flight build lock? |
| Broadcast | Is raw transaction durability separated from broadcast uncertainty? |
| Recover | Are transient observation failures kept distinct from chain failure facts? |
| Receipt upload | Can terminal rows remain observable until upload succeeds? |
| Result ACK | Is duplicate result ACK prevented across scanners and workers? |
| Origin projection | Can child flow success or failure move the parent only in allowed directions? |
| Follow-up work | Is cleanup, reclaim, or wakeup idempotent after restart? |

If a stage is touched, the matching assertion matrix must record the durable
facts, external calls, retry behavior, and invariant. If the matrix does not
already contain the case, add it in the same change.

## Resource-Specific Gate

Resource changes must explicitly check these invariants:

- Transient RPC, timeout, rate-limit, backpressure, and authorization
  observation failures do not prove chain failure.
- Pre-broadcast permanent failure and post-broadcast chain failure are separate
  terminal facts.
- Delegation and reclaim ACKs are claimed before backend calls so competing
  workers cannot send duplicates.
- Platform and local resource rows release only their matching collect or
  withdraw origin.
- A collect or withdraw origin gate is terminal once released; later stale
  resource results cannot move it backward.
- Local reclaim is not created or executed before the origin collect has a
  durable terminal fact.
- Capacity, reservation, and pending-budget calculations subtract unfinished
  same-owner work and remain valid after process restart.

## Required Test Shape

Every non-trivial flow change needs at least:

- One success-path test for the changed stage.
- One failure-path or retry-path test that asserts no forbidden side effect.
- A DB fact assertion and an external-call or scanner/queue assertion when the
  flow talks to backend, chain, or another worker.
- An assertion-matrix update naming the test entrypoint and invariant.

Use the smallest valid layer from `docs/codex/testing.md`:

- Unit for classifiers, predicates, retry waits, and stage decisions.
- Component for repository or DAO facts with temporary SQLite.
- Integration for worker/scanner/backend/chain orchestration.
- Smoke/live only for manual real-environment checks.

## Review Questions

Code review for these flows should ask:

- Which lifecycle stage changed?
- Which durable fact is the source of truth after retry or restart?
- Which side effect is protected from duplication?
- Which invariant prevents the previous class of bug?
- Which assertion-matrix entry proves the invariant is now project memory?

If these questions cannot be answered from tests and matrix entries, the change
is not ready.
