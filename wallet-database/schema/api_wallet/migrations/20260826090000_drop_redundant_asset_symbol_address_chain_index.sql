-- The unique (address, chain_code, token_address) index already serves address-scoped lookups.
-- This legacy symbol-first index is not forced by any query and duplicates that access path.
DROP INDEX IF EXISTS api_assets_symbol_address_chain_idx;
