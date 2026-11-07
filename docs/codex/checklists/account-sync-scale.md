# Account Synchronization Scale and Reuse Checklist

## When to Read

Read this checklist before broadening account query scope (for example, one
account type to all types) or changing bulk account/address synchronization,
including missing-chain supplementation and account expansion. Apply the checks
in proportion to the change; this does not require a particular batch size,
expansion state machine, per-step approval or user review of every line.

## Existing Capability Reuse

- Trace existing synchronization, expansion, derivation, registration and recovery
  paths before adding a new path. Identify concrete methods to reuse, required
  adaptations and business boundaries that prevent direct reuse.
- Avoid a parallel framework where existing capabilities can serve the flow.
  Explain why any independent path is necessary; similar execution steps alone
  do not prove that index allocation, account types or backend completion semantics
  are interchangeable.

## Scale and Failure Checks

- Estimate account/address counts, query and processing complexity, peak memory
  and backend request volume. Record assumptions and limits in `PLANS.md`.
- Assess stable pagination while data changes, including duplicates and omissions;
  request chunking alone does not establish bounded database reads.
- Bound derivation and database write batches, and registration requests by both
  item count and payload bytes. Identify where limits are enforced.
- Assess concurrent or out-of-order results, timeouts, duplicate requests and
  partial success: what is persisted, what can be retried, and which unfinished
  work is rediscovered after interruption or process restart?
- Distinguish dispatch success from registration completion. An in-memory queue
  alone does not establish persistent recovery.

## Validation and Delivery

- Follow `docs/codex/testing.md` for affected tests; reuse adequate existing
  evidence and run the smallest relevant validation set.
- Report tested sizes, resource/request bounds and failure/recovery outcomes,
  including relevant duplicate, out-of-order, partial-success and restart cases.
- Identify uncovered paths and assumptions. Do not infer large-account readiness
  from small fixtures or claim recovery solely because requests are chunked.
- Keep concrete defects, commands, results and remaining acceptance work in the
  task's `PLANS.md` and relevant assertion matrix, not in AGENTS.md or this checklist.
