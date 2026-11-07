-- Create expand_batch_item table to track individual expand items
CREATE TABLE IF NOT EXISTS expand_batch_item (
    batch_id TEXT,
    uid TEXT,
    chain_code TEXT,
    input_index INTEGER,
    status INTEGER NOT NULL DEFAULT 0,
    -- 0=Pending, 1=Creating, 2=Initing, 3=Done, 4=Failed
    retry_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TIMESTAMP,
    last_init_dispatched_at TIMESTAMP NULL,
    PRIMARY KEY (batch_id, input_index),
    FOREIGN KEY (batch_id) REFERENCES expand_batch(batch_id) ON DELETE CASCADE
);
-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_expand_batch_item_batch_id ON expand_batch_item(batch_id);
CREATE INDEX IF NOT EXISTS idx_expand_batch_item_status ON expand_batch_item(status);
CREATE INDEX IF NOT EXISTS idx_expand_batch_item_chain_status ON expand_batch_item(chain_code, status);
CREATE INDEX IF NOT EXISTS idx_expand_batch_item_last_init_dispatched ON expand_batch_item(last_init_dispatched_at);
