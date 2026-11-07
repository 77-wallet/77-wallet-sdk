-- Add migration script here
CREATE TABLE api_wallet (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 增加唯一主键
    name VARCHAR(64) NOT NULL,
    uid VARCHAR(64) NOT NULL,
    address VARCHAR(64) NOT NULL UNIQUE,
    -- address 建唯一约束即可，无需再作为主键
    phrase BLOB NOT NULL,
    -- 助记词可能会超过 64 字符
    seed BLOB,
    -- 同上，长度可能更长
    api_wallet_type INTEGER NOT NULL,
    binding_address VARCHAR(64),
    merchant_id TEXT,
    app_id TEXT NULL,
    sn TEXT NULL,
    status INTEGER NOT NULL,
    is_init INTEGER NOT NULL,
    import_stage INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);
CREATE TRIGGER IF NOT EXISTS trg_api_wallet_type_insert_check BEFORE
INSERT ON api_wallet FOR EACH ROW
    WHEN NEW.api_wallet_type NOT IN (1, 2) BEGIN
SELECT RAISE(ABORT, 'invalid api_wallet_type');
END;
CREATE TRIGGER IF NOT EXISTS trg_api_wallet_type_update_check BEFORE
UPDATE OF api_wallet_type ON api_wallet FOR EACH ROW
    WHEN NEW.api_wallet_type NOT IN (1, 2) BEGIN
SELECT RAISE(ABORT, 'invalid api_wallet_type');
END;
CREATE INDEX api_wallet_uid_idx ON api_wallet (uid);
CREATE INDEX api_wallet_merchant_id_idx ON api_wallet (merchant_id);
CREATE INDEX api_wallet_app_id_idx ON api_wallet (app_id);
