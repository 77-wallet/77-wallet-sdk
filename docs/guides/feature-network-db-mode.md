# Feature-Network + Single DB Mode

## Network selection
- `prod` feature => `mainnet`
- `dev` / `test` feature => `testnet`
- Runtime config does **not** switch network.

## DB layout
- Single DB directory: `<root_dir>/db`
- No automatic `db/mainnet` / `db/testnet` migration.
- If legacy namespaced directories exist, startup logs warning only.
- If you need to run both mainnet and testnet at the same time, use different `root_dir` values.

## Manual migration guidance
If you need old data from legacy namespaced DB directories, migrate manually.

Mainnet runtime (prod):
```bash
cp -av <root_dir>/db/mainnet/*.db <root_dir>/db/
```

Testnet runtime (dev/test):
```bash
cp -av <root_dir>/db/testnet/*.db <root_dir>/db/
```

Do not mix mainnet and testnet DB files into the same `db/` at the same time.

## Coin default config
- `wallet-api/data/config/coin.mainnet.toml`
- `wallet-api/data/config/coin.testnet.toml`
- `wallet-api/data/config/coin.toml` is deprecated and only documents behavior.
