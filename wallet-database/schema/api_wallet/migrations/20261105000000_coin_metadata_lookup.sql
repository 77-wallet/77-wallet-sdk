-- Match EVM metadata comparisons without changing uniqueness or stored spelling.
-- The existing address-first EVM identity index cannot serve a token-wide update.
CREATE INDEX api_assets_metadata_nocase_idx
ON api_assets(chain_code, token_address COLLATE NOCASE);

-- Prevent the correlated label-difference check from scanning the whole catalog
-- for every asset, including no-op catalog replays.
CREATE INDEX api_coin_metadata_nocase_idx
ON api_coin(chain_code, token_address COLLATE NOCASE);
