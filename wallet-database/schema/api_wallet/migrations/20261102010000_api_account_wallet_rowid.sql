-- id is INTEGER PRIMARY KEY (the rowid). SQLite already appends it to
-- ordinary index entries; storing it explicitly duplicates it on disk.
-- Keep the existing name and wallet/id seek order for deletion paging.
DROP INDEX IF EXISTS api_account_wallet_id_idx;
CREATE INDEX api_account_wallet_id_idx ON api_account (wallet_address);
