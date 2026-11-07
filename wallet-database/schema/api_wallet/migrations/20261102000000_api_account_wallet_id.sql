-- Bound deletion keyset reads to one wallet without walking unrelated rowids.
CREATE INDEX IF NOT EXISTS api_account_wallet_id_idx
ON api_account (wallet_address, id);
