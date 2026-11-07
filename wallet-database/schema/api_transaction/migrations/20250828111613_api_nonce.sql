-- Add migration script here

CREATE TABLE api_nonce
(
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    from_addr        VARCHAR(64)             NOT NULL,
    chain_code       VARCHAR(64)             NOT NULL,
    nonce  INTEGER,
    created_at       TIMESTAMP               NOT NULL,
    updated_at       TIMESTAMP
);

CREATE UNIQUE INDEX api_nonce_from ON api_nonce (from_addr, chain_code);