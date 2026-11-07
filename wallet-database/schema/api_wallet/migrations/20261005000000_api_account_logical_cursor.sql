-- Correlated earliest-active probes must not scan every account in a wallet.
CREATE INDEX IF NOT EXISTS api_account_logical_status_id_idx
ON api_account (wallet_address, account_id, api_wallet_type, status);
