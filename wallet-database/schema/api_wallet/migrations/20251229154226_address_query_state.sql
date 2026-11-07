-- Add migration script here
-- 地址查询状态表（uid + chain 维度）
CREATE TABLE IF NOT EXISTS address_query_state (
    uid TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    status INTEGER NOT NULL,
    -- 0 = running    （正在查询地址）
    -- 1 = done       （查询完成，地址已全部入库）
    -- 2 = failed     （查询失败，需要人工或重试）
    last_page INTEGER NOT NULL DEFAULT 0,
    total_remote INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TIMESTAMP NULL,
    PRIMARY KEY (uid, chain_code)
);
CREATE INDEX IF NOT EXISTS idx_address_query_state_status ON address_query_state(status);
