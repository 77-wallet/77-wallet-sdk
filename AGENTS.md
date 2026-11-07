# AGENTS.md (root)

## Boundaries

- Workspace crates: wallet-api / wallet-transport-backend / wallet-database / wallet-oss / wallet-tree / wallet-ecdh
- Docs under `docs/`; tests under each crate `tests/`
- Write build outputs only to `target/`; do not commit temp runtime files

## Rule Discovery

- Submodules have their own `AGENTS.md`
- Always resolve and follow nearest rules in `root -> leaf` order

## Mandatory References

- Before implementation, follow `docs/codex/workflows.md#step-1--scope` and read the applicable confirmed context in `docs/codex/architecture.md#confirmed-project-context`; clarify consequential unknowns and record user-confirmed facts for reuse.
- Testing rules are defined in `docs/codex/testing.md`.
- PR acceptance criteria are defined in `docs/codex/checklists/pr-definition-of-done.md`.
- Commit message rules are defined in `docs/codex/commit-message.md`.
- Codex must follow these documents when generating or modifying tests.
- Before non-trivial test changes, read `docs/codex/testing.md` first.
- Every PR must pass `docs/codex/checklists/pr-definition-of-done.md`
- For non-trivial tasks, create or update `PLANS.md` before implementation

## Change Size Gate

- If estimated change spans `>=2` crates or `>=10` files, split the change into small implementation and validation batches before starting.
- Update `PLANS.md` with:
  - batch scope
  - target validation commands
  - batch completion checkpoint and review-stage boundary or actual blocker
- Each batch should focus on one module and one flow, and keep verification to the smallest affected command set.
- For test-first tasks, do not expand architecture refactors until the target regression tests are added and passing for that batch.

## Review Stages and Authorized Continuation

- After the user approves a plan, implement and validate small batches within the same repository and review stage. Treat batch completion as a checkpoint: inspect the diff and validation results, then continue toward the approved goal without waiting for another "continue" merely because a batch ended. In `PLANS.md`, distinguish batch checkpoints from review-stage boundaries and actual blockers.
- Preserve review before commit. At the end of a review stage, present the changes, validation evidence, and remaining limitations, and leave the changes uncommitted until the user explicitly approves the commit. Do not treat implementation approval or "continue" as commit approval.
- Preserve dependency review stages when a fixed revision is needed: Core review and approved commit precede the SDK's fixed Core hash; SDK review and approved commit precede the plugin's fixed SDK hash. If a dependency must be available remotely, use an authorized push or obtain push approval. Repository changes alone do not introduce another confirmation requirement, and already granted authorization need not be requested again.
- Commit, push, merge, and release are separate permissions. Approval for one does not authorize the others; "continue" does not authorize push, merge, or release. Check helper targets for these actions before running them.
- Ask for confirmation when business meaning, scope, or required permissions change, or a necessary business decision is missing. Continue independent work that remains authorized. Explicit user pauses and pending product decisions take precedence over automatic continuation.
- If validation or the environment repeatedly fails without new evidence, report the blocker, attempted checks, and the decision or input needed instead of repeating the same failing action. Fix routine failures within the approved scope when new evidence provides a path forward.
- Automatic continuation does not authorize spending funds, signing or broadcasting real transactions, or changing production. Preserve existing security rules and require authorization covering those actions.

## Async State Consistency

- When changing asynchronous queries or shared-state updates, assess how concurrency, duplicate or out-of-order results, timeout/cancellation, and partial failures could affect state consistency, in proportion to the change's risk.
- Provide relevant validation evidence and identify uncovered paths; use controlled-timing tests when applicable. Reuse adequate existing evidence where appropriate rather than requiring new tests for every change. This assessment adds no step-by-step approval gate and does not prescribe serialization, versioning, or another implementation strategy.

## Account Scale and Existing Capability Reuse

- Prefer existing capabilities; assess scale before broadening account queries or changing bulk account/address synchronization. For these changes, read `docs/codex/checklists/account-sync-scale.md` before implementation.

## Pre-delivery Independent Review

- Before delivery, the implementation/coordinating task must initiate independent read-only review following `docs/codex/workflows.md#pre-delivery-independent-review`. Report new review findings, their impact and the proposed correction before fixing them; proceed only within the user's applicable approval. Already approved corrections do not require approval again. The reviewer reports findings only.

## Security

- Never commit or print private keys, mnemonics, credentials, or production config

## Commit Message Policy

- Follow `docs/codex/commit-message.md`.
