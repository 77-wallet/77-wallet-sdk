CREATE TABLE asset_query_state (
    uid TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    page INTEGER NOT NULL,
    status INTEGER NOT NULL,
    -- 0 = pending, 1 = running, 2 = done, 3 = failed
    index_list_json TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TIMESTAMP NULL,
    PRIMARY KEY (uid, chain_code, page)
);

CREATE INDEX idx_asset_query_state_status ON asset_query_state(status);
CREATE INDEX idx_asset_query_state_created_at ON asset_query_state(created_at);
