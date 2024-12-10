-- Add migration script here
CREATE TABLE multisig_queue(
    id VARCHAR(64) PRIMARY KEY,
    from_addr VARCHAR(128) NOT NULL,
    to_addr VARCHAR(128) NOT NULL,
    msg_hash VARCHAR(128) NOT NULL,
    tx_hash VARCHAR(128) NOT NULL,
    chain_code VARCHAR(32) NOT NULL,
    symbol VARCHAR(32) NOT NULL,
    token_addr VARCHAR(128),
    value VARCHAR(128) NOT NULL,
    expiration INTEGER DEFAULT 0,
    account_id  VARCHAR(64) NUll DEFAULT "",
    raw_data TEXT,
    status INTEGER NOT NULL,
    fail_reason VARCHAR(256) DEFAULT "",
    notes TEXT NOT NULL,
    is_del integer NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);
CREATE INDEX msg_hash ON multisig_queue (msg_hash);