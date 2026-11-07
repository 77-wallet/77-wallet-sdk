-- Improve join/selectivity for wallet asset aggregation and asset-calc lookups.
-- Support joins filtered by address + chain + status (api_account side).
CREATE INDEX IF NOT EXISTS api_account_address_chain_status_idx
ON api_account(address, chain_code, status, wallet_address);

-- Support token-keyset lookups and keep address available for the subsequent account join.
CREATE INDEX IF NOT EXISTS api_assets_symbol_chain_token_status_idx
ON api_assets(symbol, chain_code, token_address, status, address);
