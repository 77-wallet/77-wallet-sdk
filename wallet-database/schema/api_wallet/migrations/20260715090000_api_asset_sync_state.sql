CREATE TABLE IF NOT EXISTS api_asset_sync_state (
    address TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    token_address TEXT NOT NULL DEFAULT '',
    status INTEGER NOT NULL,
    -- 0 = pending, 1 = running, 2 = done, 3 = failed
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TIMESTAMP NULL,
    PRIMARY KEY (address, chain_code, token_address)
);

CREATE INDEX IF NOT EXISTS idx_api_asset_sync_state_status
    ON api_asset_sync_state(status, updated_at, created_at);
