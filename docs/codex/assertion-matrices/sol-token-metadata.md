# Solana custom-token metadata adapter

Scope: the SDK transaction adapter's decimals/symbol/name calls used by custom-token lookup.
These tests do not exercise database lookup, coin insertion, or the APP UI.

| Scenario | Expected result | Invariant |
| --- | --- | --- |
| USDG Token-2022 mint with self-pointer metadata | decimals 6, symbol USDG, name GlobalDollar (existing normalization) | Only supply and mint reads; no Metaplex lookup or writes |
| Mint account absent | Preserve mint-account-not-found error | Stop at mint read; do not request unrelated metadata |

Offline command (the explicit proxy exclusions keep the real RPC client on loopback):

```sh
NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost \
  cargo test -p wallet-api --test sol_token_metadata --features integration-tests
```

The public mint fixture was captured on 2026-09-16. The RPC runs on loopback and
requires no wallet initialization, credentials, backend, database, or live chain.
