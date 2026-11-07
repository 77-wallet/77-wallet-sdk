-- Add migration script here
CREATE TABLE permission(
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    grantor_addr VARCHAR(128) NOT NULL,
    types VARCHAR(128) NOT NULL,
    active_id INTEGER NOT NULL,
    threshold INTEGER NOT NULL,
    member INTEGER NOT NULL,
    chain_code VARCHAR(32) NOT NULL,
    operations VARCHAR(128),
    is_del INTEGER DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);
CREATE INDEX grantor_active ON permission (grantor_addr,active_id);