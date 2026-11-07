# Commit Message Guide

This repository uses Conventional Commits and enforces it with `commitlint`.

## Required Format

Use:

`type(scope): short summary`

Example:

`feat(api-wallet): support withdrawal wallet binding`

## Rules

- Use imperative mood in summary (for example: `add`, `fix`, `refactor`, `remove`).
- Keep header length within 72 characters.
- Include module scope when possible.
- Do not use generic messages such as `update code`.

## Allowed Types

- `feat`
- `fix`
- `refactor`
- `test`
- `docs`
- `chore`
- `perf`

## Preferred Scopes

- `api-wallet`
- `context`
- `service`
- `domain`
- `repo`
- `task-queue`
- `tests`
- `manager`

You may use other module scopes when needed. Prefer kebab-case, for example:

- `wallet-transport`
- `build-system`
- `infra`

## Good Examples

- `feat(api-wallet): support withdrawal wallet binding`
- `test(api-wallet): add scan_bind smoke test`
- `refactor(context): inject ApiWalletBackend trait`
- `fix(task-queue): prevent duplicate task execution`
- `docs(tests): add api-wallet testing playbook`

## Bad Examples

- `update code`
- `fix: update code`
- `Configure commitlint hooks`
