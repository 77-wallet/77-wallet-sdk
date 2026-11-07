# API Wallet Transaction Worker Evolution

## Archived implementation

The actor-based collect, fee-collection, and withdrawal workers that preceded the
Shadow pipeline are preserved in Git at:

```text
branch: archive/pre-shadow-legacy-workers-20260917
commit: ee0dddca35df87c877eaf76f65f339754688e9e2
```

Review the implementation without changing the current checkout:

```bash
git show archive/pre-shadow-legacy-workers-20260917:wallet-api/src/infrastructure/api_trans/collect/legacy/process_collect_tx_send.rs
git show archive/pre-shadow-legacy-workers-20260917:wallet-api/src/infrastructure/api_trans/collect_fee/legacy/process_fee_tx_send.rs
git show archive/pre-shadow-legacy-workers-20260917:wallet-api/src/infrastructure/api_trans/withdraw/legacy/process_withdraw_tx_send.rs
```

To inspect the complete tree, create a temporary worktree from the archive branch.

## Legacy design

Each transaction family used a long-running actor pipeline with bounded channels:

1. A send worker selected pending work, acquired an address lock and a global permit,
   built the transaction, and broadcast it.
2. A report worker sent transaction results to the backend.
3. A confirm-report worker retried confirmation acknowledgements.
4. Process-local sets, semaphores, channels, and retry timing coordinated the stages.

This design kept the initial implementation direct and made the execution order easy
to follow inside one process. Address-level semaphores prevented two transactions for
the same account from running concurrently, while global semaphores limited total work.

## Why the design changed

The process-local actor pipeline became difficult to recover after interruption. A
channel message, in-memory guard, or worker-local retry timer was not a durable fact.
As transaction handling gained resource delegation, backend acknowledgements, nonce
reservation, uncertain broadcast recovery, and multiple fast paths, deciding the next
step from worker-local state also risked duplicate side effects or stalled work.

The Shadow pipeline instead derives work from persisted facts. Scanners identify the
next eligible stage, workers own narrowly defined side effects, and advancement can be
repeated after restart. Database predicates and persisted acknowledgements protect the
same transition when periodic scans and fast-path notifications race.

## Preserved ideas

The migration did not discard every legacy mechanism. Address-level serialization is
still required by the Shadow workers, and the application still exposes handles that
own each Shadow actor system. These active components were moved out of `legacy`
before the unreachable workers were removed.

## Comparison guide

When reviewing the evolution, compare these concerns:

| Concern | Legacy actor pipeline | Shadow pipeline |
| --- | --- | --- |
| Source of truth | Channels and process-local worker state | Persisted transaction facts |
| Restart recovery | Reconstruct work through bespoke worker logic | Scanner derives eligible work again |
| Retry scheduling | Worker loops and local timers | Persisted state plus scanner/fast-path admission |
| Side-effect ownership | Large send/report workers | Stage-specific workers and guarded transitions |
| Concurrency | Address and global semaphores | Address/resource gates plus database predicates |
| Auditability | Active and inactive paths coexist | Current production path remains in the source tree |

The archived branch is the authoritative source for exact historical code. This note
records the design intent so future review does not need to infer the migration solely
from a large diff.
