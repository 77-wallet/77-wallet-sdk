-- Read-only label source. Keep asset-specific fields and caller visibility policies.
-- Only a missing catalog identity falls back to the legacy cached labels.
-- The two mutually exclusive joins match the existing EVM/binary identity guards.
CREATE VIEW api_assets_with_coin_labels AS
SELECT a.id,
       CASE WHEN evm.id IS NOT NULL THEN evm.name
            WHEN other.id IS NOT NULL THEN other.name ELSE a.name END AS name,
       CASE WHEN evm.id IS NOT NULL THEN evm.symbol
            WHEN other.id IS NOT NULL THEN other.symbol ELSE a.symbol END AS symbol,
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
