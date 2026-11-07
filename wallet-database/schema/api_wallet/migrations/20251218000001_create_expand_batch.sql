-- Add migration script here
-- 创建扩容批次表，用于跟踪扩容进度
CREATE TABLE IF NOT EXISTS expand_batch (
    uid TEXT NOT NULL,
    batch_id TEXT PRIMARY KEY,
    serial_no TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    total_count INTEGER NOT NULL,
    finished_count INTEGER NOT NULL DEFAULT 0 CHECK (finished_count <= total_count),
    retry_count INTEGER NOT NULL DEFAULT 0,
    status INTEGER NOT NULL DEFAULT 0,
    -- 0=running, 1=done
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TIMESTAMP,
    expand_complete_at TIMESTAMP,
    local_complete_at TIMESTAMP
);
-- 添加索引以提高查询性能
CREATE INDEX IF NOT EXISTS idx_expand_batch_chain_status ON expand_batch(chain_code, status);
CREATE INDEX IF NOT EXISTS idx_expand_batch_created_at ON expand_batch(created_at);
