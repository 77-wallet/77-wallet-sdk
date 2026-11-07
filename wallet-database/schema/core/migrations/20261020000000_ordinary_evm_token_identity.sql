-- EVM contracts are one identity irrespective of casing. Balance rows are
-- snapshots, never additive. Prefer the existing canonical spelling, otherwise
-- the newest cached row. A durable marker requests an authoritative refresh.
CREATE TABLE IF NOT EXISTS ordinary_asset_identity_refresh (
    address TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    token_address TEXT NOT NULL,
    dispatch_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (address, chain_code, token_address)
);

CREATE INDEX IF NOT EXISTS ordinary_identity_refresh_dispatch_idx
ON ordinary_asset_identity_refresh(dispatch_order, address, chain_code, token_address);

-- Preserve the core schema's existing symbol-alias dimension.
CREATE TEMP TABLE ordinary_evm_asset_rank AS
SELECT rowid AS source_rowid,
       row_number() OVER (
           PARTITION BY address, chain_code, lower(token_address), symbol
           ORDER BY (token_address = lower(token_address)) DESC,
                    julianday(COALESCE(updated_at, created_at)) DESC, rowid DESC
       ) AS priority,
       count(*) OVER (
           PARTITION BY address, chain_code, lower(token_address), symbol
       ) AS copies
FROM assets
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
  AND token_address IS NOT NULL AND token_address <> '';

INSERT OR IGNORE INTO ordinary_asset_identity_refresh(address, chain_code, token_address)
SELECT a.address, a.chain_code, lower(a.token_address)
FROM assets a JOIN ordinary_evm_asset_rank r ON r.source_rowid = a.rowid
WHERE r.priority = 1 AND r.copies > 1;

DELETE FROM assets WHERE rowid IN (
    SELECT source_rowid FROM ordinary_evm_asset_rank WHERE priority > 1
);
DROP TABLE ordinary_evm_asset_rank;

CREATE TEMP TABLE ordinary_evm_coin_rank AS
SELECT rowid AS source_rowid,
       row_number() OVER (
           PARTITION BY chain_code, lower(token_address), symbol
           ORDER BY (token_address = lower(token_address)) DESC,
                    julianday(COALESCE(updated_at, created_at)) DESC, rowid DESC
       ) AS priority
FROM coin
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
  AND token_address IS NOT NULL AND token_address <> '';

DELETE FROM coin WHERE rowid IN (
    SELECT source_rowid FROM ordinary_evm_coin_rank WHERE priority > 1
);
DROP TABLE ordinary_evm_coin_rank;

UPDATE assets SET token_address = lower(token_address)
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
  AND token_address <> lower(token_address);
UPDATE coin SET token_address = lower(token_address)
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
  AND token_address <> lower(token_address);

CREATE UNIQUE INDEX IF NOT EXISTS ordinary_assets_evm_identity_idx
ON assets(address, chain_code, lower(token_address), symbol)
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
  AND token_address IS NOT NULL AND token_address <> '';
CREATE UNIQUE INDEX IF NOT EXISTS ordinary_coin_evm_identity_idx
ON coin(chain_code, lower(token_address), symbol)
WHERE chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
  AND token_address IS NOT NULL AND token_address <> '';

-- Existing backend/RPC update paths clear the marker only after a successful
-- balance write, atomically with that write. Initialization does not set balance.
-- Catalog precision conversions also write balance but are not remote observations.
CREATE TRIGGER IF NOT EXISTS ordinary_asset_identity_refreshed
AFTER UPDATE OF balance ON assets
WHEN NEW.decimals = OLD.decimals
BEGIN
    DELETE FROM ordinary_asset_identity_refresh
    WHERE address = NEW.address AND chain_code = NEW.chain_code
      AND token_address = NEW.token_address;
END;
