-- Add migration script here
CREATE TABLE api_assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name VARCHAR(64) NOT NULL,
    symbol VARCHAR(64) NOT NULL,
    decimals INTEGER NOT NULL,
    address VARCHAR(28) NOT NULL,
    token_address VARCHAR(128) DEFAULT "" NOT NULL,
    protocol VARCHAR(20) NULL,
    chain_code VARCHAR(32) NOT NULL,
    status INTEGER NOT NULL,
    is_multisig INTEGER DEFAULT 0 NOT NULL,
    balance VARCHAR(256) DEFAULT '0' NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);
CREATE UNIQUE INDEX api_assets_symbol_address_chain_token_idx ON api_assets (address, chain_code, token_address);
CREATE INDEX api_assets_symbol_address_chain_idx ON api_assets (symbol, address, chain_code);
-- 聚合查询常用过滤
CREATE INDEX api_assets_address_chain_status_idx ON api_assets (address, chain_code, status);
CREATE INDEX api_assets_chain_token_symbol_status_idx ON api_assets (chain_code, token_address, symbol, status);
-- 创建api_assets的覆盖索引
CREATE INDEX IF NOT EXISTS api_assets_join_cover_idx ON api_assets(
    address,
    chain_code,
    status,
    token_address,
    balance
);
