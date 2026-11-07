-- Add migration script here
CREATE TABLE api_account (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    name VARCHAR(64) NOT NULL,
    address VARCHAR(128) NOT NULL,
    pubkey VARCHAR(128),
    address_type VARCHAR(64) DEFAULT '',
    wallet_address VARCHAR(128) NOT NULL,
    derivation_path VARCHAR(32),
    derivation_path_index INTEGER,
    chain_code VARCHAR(32) NOT NULL,
    uid VARCHAR(64) NOT NULL DEFAULT "",
    api_wallet_type INTEGER NOT NULL,
    status INTEGER NOT NULL,
    is_init INTEGER NOT NULL,
    is_expand INTEGER NOT NULL DEFAULT 0,
    is_used Boolean NOT NULL DEFAULT false,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);
CREATE TRIGGER IF NOT EXISTS trg_api_account_type_insert_check BEFORE
INSERT ON api_account FOR EACH ROW
    WHEN NEW.api_wallet_type NOT IN (1, 2) BEGIN
SELECT RAISE(ABORT, 'invalid api_wallet_type');
END;
CREATE TRIGGER IF NOT EXISTS trg_api_account_type_update_check BEFORE
UPDATE OF api_wallet_type ON api_account FOR EACH ROW
    WHEN NEW.api_wallet_type NOT IN (1, 2) BEGIN
SELECT RAISE(ABORT, 'invalid api_wallet_type');
END;
-- 唯一索引：避免同 address + chain_code + address_type 三元组重复
CREATE UNIQUE INDEX api_account_address_chain_code_idx ON api_account (address, chain_code, address_type);
-- 常用查询 + 覆盖 range
CREATE INDEX api_account_wallet_chain_account_idx ON api_account (wallet_address, chain_code, account_id);
-- expand_batch_item 事实状态查询使用
CREATE INDEX api_account_uid_chain_index_idx ON api_account (uid, chain_code, derivation_path_index);
CREATE INDEX api_account_uid_chaincode_idx ON api_account (uid, chain_code);
CREATE INDEX idx_api_account_chain_status ON api_account (chain_code, status);
-- 创建api_account的钱包状态索引
CREATE INDEX IF NOT EXISTS api_account_wallet_status_idx ON api_account(
    wallet_address,
    status,
    chain_code,
    address
);