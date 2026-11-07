-- Add migration script here

CREATE TABLE api_chain
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        VARCHAR(64) NOT NULL,
    chain_code  VARCHAR(32) NOT NULL,
    node_id     VARCHAR(64),
    node_bind_type INTEGER NOT NULL DEFAULT 0,
    protocols   TEXT        NOT NULL,
    main_symbol VARCHAR(16) NOT NULL,
    status      INTEGER     NOT NULL,
    created_at  TIMESTAMP   NOT NULL,
    updated_at  TIMESTAMP
);

CREATE UNIQUE INDEX api_chain_chain_code_idx ON api_chain (chain_code);
