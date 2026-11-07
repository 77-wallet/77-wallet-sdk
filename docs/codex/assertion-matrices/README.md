# Assertion Matrices

This directory records the concrete assertion matrix for each standardized
test flow.

Use one file per business module or high-risk flow. Each matrix must state:

- test layer
- executable test entrypoint
- expected backend calls
- expected database facts
- failure invariants
- known coverage gaps

When a PR changes a flow listed here, update the matching matrix in the same
PR. If the flow is not listed yet, add a small matrix for the changed flow
instead of expanding unrelated modules.

For API wallet transaction-like flows, first classify the changed lifecycle
stage with `docs/codex/api-wallet-flow-maintenance.md`. The matrix entry must
name the durable facts, external side effects, retry behavior, and invariant for
that stage. Resource delegation/reclaim changes use the same standard as
collect, withdraw, and collect-fee; resource must not be treated as an
untracked side module.
