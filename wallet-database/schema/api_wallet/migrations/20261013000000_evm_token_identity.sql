-- Future EVM duplicate prevention. Existing new-chain test data was cleaned
-- once on the sole testing device; no historical data cleanup at startup.

CREATE UNIQUE INDEX IF NOT EXISTS api_coin_evm_identity_idx
ON api_coin(chain_code, lower(token_address))
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll');

CREATE UNIQUE INDEX IF NOT EXISTS api_assets_evm_identity_idx
ON api_assets(address, chain_code, lower(token_address))
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll');
