-- The retained (uid, chain_code, derivation_path_index) index supplies the
-- same UID/chain prefix, and additionally supports expansion fact lookups.
-- Avoid maintaining a second nonunique prefix on every account write/delete.
DROP INDEX IF EXISTS api_account_uid_chaincode_idx;
