-- Preserve missing catalog identities before removing per-address cached labels.
-- Never overwrite catalog metadata. The newest asset snapshot wins; id breaks ties.
-- Synthetic metadata uses epoch times so it cannot advance backend sync watermarks.
WITH missing AS (
    SELECT a.*,coalesce(nullif(a.updated_at,''),a.created_at) AS label_time,
           CASE WHEN a.chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll') THEN lower(a.token_address) ELSE a.token_address END AS identity_token
    FROM api_assets a
LEFT JOIN api_coin evm INDEXED BY api_coin_metadata_nocase_idx
  ON a.chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
 AND evm.chain_code=a.chain_code
 AND evm.token_address=a.token_address COLLATE NOCASE
LEFT JOIN api_coin other INDEXED BY api_coin_chain_code_token_address_idx
  ON a.chain_code NOT IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
 AND other.chain_code=a.chain_code
 AND other.token_address=a.token_address
    WHERE evm.id IS NULL AND other.id IS NULL
), sort_keys AS (
    -- Strip fractional seconds before UTC conversion: SQLite's date functions
    -- round submilliseconds. Compare the original nanoseconds separately.
    SELECT *,
           unixepoch(CASE WHEN length(label_time)=10 THEN label_time ELSE
               substr(label_time,1,19) || CASE
                   WHEN length(label_time)>=25 AND substr(label_time,-6,1) IN ('+','-') THEN substr(label_time,-6)
                   ELSE 'Z' END END) AS whole_second,
           CASE WHEN substr(label_time,20,1)='.' THEN
               substr(substr(label_time,21,length(label_time)-20-CASE
                   WHEN substr(label_time,-1)='Z' THEN 1
                   WHEN length(label_time)>=25 AND substr(label_time,-6,1) IN ('+','-') THEN 6
                   ELSE 0 END) || '000000000',1,9)
               ELSE '000000000' END AS fractional_second
    FROM missing
)
INSERT INTO api_coin(chain_code,token_address,name,symbol,protocol,decimals,created_at,updated_at)
SELECT chain_code,identity_token,name,symbol,protocol,decimals,
       '1970-01-01T00:00:00Z','1970-01-01T00:00:00Z'
FROM (
    SELECT *,row_number() OVER (
        PARTITION BY chain_code,identity_token
        ORDER BY whole_second DESC,fractional_second DESC,id DESC
    ) AS ordinal FROM sort_keys
) WHERE ordinal=1;

DROP VIEW api_assets_with_coin_labels;
DROP INDEX api_assets_chain_token_symbol_status_idx;
DROP INDEX api_assets_symbol_chain_token_status_idx;
ALTER TABLE api_assets DROP COLUMN name;
ALTER TABLE api_assets DROP COLUMN symbol;

-- Retain raw refresh rows if a later operation removes their catalog entry.
-- Old orphan labels were recovered above; no cached columns remain.
CREATE VIEW api_assets_with_coin_labels AS
SELECT a.id,
       CASE WHEN evm.id IS NOT NULL THEN evm.name
            WHEN other.id IS NOT NULL THEN other.name ELSE '' END AS name,
       CASE WHEN evm.id IS NOT NULL THEN evm.symbol
            WHEN other.id IS NOT NULL THEN other.symbol ELSE '' END AS symbol,
       a.decimals, a.address, a.token_address, a.protocol, a.chain_code,
       a.status, a.is_multisig, a.balance, a.created_at, a.updated_at
FROM api_assets a
LEFT JOIN api_coin evm INDEXED BY api_coin_metadata_nocase_idx
  ON a.chain_code IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
 AND evm.chain_code=a.chain_code
 AND evm.token_address=a.token_address COLLATE NOCASE
LEFT JOIN api_coin other INDEXED BY api_coin_chain_code_token_address_idx
  ON a.chain_code NOT IN ('eth','bnb','polygon','avalanche','fantom','arbitrum','base','scroll')
 AND other.chain_code=a.chain_code
 AND other.token_address=a.token_address;
