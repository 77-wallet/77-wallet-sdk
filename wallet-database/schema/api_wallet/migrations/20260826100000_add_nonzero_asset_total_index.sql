-- Wallet totals only read active assets with a nonzero balance. Keeping those rows in a
-- partial covering index lets large-wallet totals start from the sparse result set instead
-- of probing every asset row owned by every account.
CREATE INDEX IF NOT EXISTS api_assets_nonzero_wallet_total_idx
ON api_assets(address, chain_code, token_address, balance)
WHERE status = 1 AND balance != '0';
