# Wallet SDK

`77_wallet_sdk` is a Rust workspace for wallet-related infrastructure. The repository is organized around a small set of focused crates for business orchestration, persistence, transport, object storage, key derivation, and ECDH support.

This README is intentionally repo-oriented: it describes the workspace as it exists today, so contributors can find the right crate quickly and verify changes with the right command set.

## Start Here

If you are new to the repository, these are the most useful entry points:

- `wallet-api`: wallet business orchestration, including accounts, transactions, collection, and withdrawals.
- `wallet-database`: persistence and migration logic for SQLite / SQLx-backed storage.
- `wallet-transport-backend`: transport-layer implementations and backend behavior.
- `wallet-tree`: key tree, derivation path, and address-related logic.
- `wallet-oss`: object storage helpers and client wrappers.
- `wallet-ecdh`: ECDH negotiation and encryption helpers.

There is also a `wallet-example/` package in the repository tree. It is not part of the root workspace members list, so treat it as a standalone example package.

## Workspace Layout

| Crate | Purpose |
| --- | --- |
| `wallet-api` | Business orchestration layer for wallet flows. |
| `wallet-database` | Database access, schema, and migrations. |
| `wallet-transport-backend` | Transport backend implementation. |
| `wallet-oss` | Object storage utilities. |
| `wallet-tree` | Key tree and address derivation. |
| `wallet-ecdh` | ECDH and crypto helper flows. |

## Quick Commands

Use crate-specific commands when possible so feedback stays fast and local to the change.

```sh
cargo check --workspace
cargo test -p wallet-api
cargo test -p wallet-database
cargo test -p wallet-transport-backend
```

If you are changing wallet business flows, start with `wallet-api`. If you are changing migrations or schema logic, start with `wallet-database`.

## Development Rules

- Read the nearest `AGENTS.md` before editing a crate.
- Follow `docs/codex/testing.md` for test scope and offline expectations.
- Follow `docs/codex/checklists/pr-definition-of-done.md` before opening a PR.
- Keep build output in `target/`; do not commit temporary runtime files.
- This workspace pulls in several git dependencies, so the first build or a branch switch may need extra fetch time.

## Repository Map

- `docs/codex/` for repo-level rules, test guidance, and PR checklist
- `docs/README.md` for the documentation index
- `docs/protocols/` for MQTT and external protocol references
- `docs/reference/` for lookup tables such as error codes and formulas
- `docs/guides/` for focused feature guides
- `wallet-api/` for orchestration and domain-facing business logic
- `wallet-database/` for data access and migrations
- `wallet-transport-backend/` for transport backends
- `wallet-oss/` for object storage integrations
- `wallet-tree/` for derivation and key-tree utilities
- `wallet-ecdh/` for ECDH and crypto helpers
- `wallet-example/` for sample usage

## Maintenance Notes

- When workspace membership changes, update the crate list above so the README stays accurate.
- When you add a new test flow, prefer placing the expectation notes in `docs/codex/` rather than expanding the root AGENTS file.
- If you change a module’s public shape, update the README section that points readers to the right starting crate.


## Core revision and Sui transport

The Core workspace dependencies are pinned together to
`5486d9268b9c6fd04e5c54e25d4816ae84618fe5`. Sui uses the modular 0.4.0 SDK
and full-node gRPC. Both ordinary-wallet and API-wallet adapters pass the node
URL and authentication headers to Core's fallible Provider constructor. Node
configuration must supply a gRPC-capable endpoint; existing JSON-RPC URLs are
not automatically converted. Headers must be valid ASCII gRPC metadata.

Construct adapters inside a Tokio runtime. Sui uses a 30-second response-header
wait and 30-second response-body idle limit, plus the client's default connect
limit. These are separate phase limits, not the previous HTTP overall timeout.
No automatic submission retry is added: a transport error after submission can
leave the result uncertain, so existing recovery must observe the digest before
any retry. Real endpoint/authentication, simulation and submission remain manual
acceptance checks; constructor tests do not establish live-node compatibility.
