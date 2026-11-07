-- Create expand_notify_state table to track per-page notify progress
CREATE TABLE IF NOT EXISTS expand_notify_state (
    uid TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    last_notified_page INTEGER NOT NULL DEFAULT -1,
    updated_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    PRIMARY KEY (uid, chain_code)
);

CREATE INDEX IF NOT EXISTS idx_expand_notify_state_uid_chain ON expand_notify_state(uid, chain_code);
