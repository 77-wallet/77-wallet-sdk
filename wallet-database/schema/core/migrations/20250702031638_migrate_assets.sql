-- Add migration script here
CREATE TABLE IF NOT EXISTS assets_v2 (
    name TEXT NOT NULL,
    symbol TEXT NOT NULL,
    decimals INTEGER NOT NULL,
    address TEXT NOT NULL,
    token_address TEXT,
    protocol TEXT,
    chain_code TEXT NOT NULL,
    status INTEGER NOT NULL,
    is_multisig INTEGER NOT NULL DEFAULT 0,
    balance TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT
);

INSERT INTO assets_v2 (
    name, symbol, decimals, address, token_address, protocol,
    chain_code, status, is_multisig, balance,
    created_at, updated_at
)
SELECT
    name, symbol, decimals, address, token_address, protocol,
    chain_code, status, is_multisig, balance,
    created_at, updated_at
FROM assets;

-- 3. 删除旧表
DROP TABLE assets;

-- 4. 重命名新表为原表名
ALTER TABLE assets_v2 RENAME TO assets;

-- 5. 创建唯一索引（代替旧主键）
CREATE UNIQUE INDEX IF NOT EXISTS uniq_assets_composite
ON assets(address, chain_code,token_address,symbol);
