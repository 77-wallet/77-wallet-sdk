-- Add migration script here
CREATE TABLE
    chain (
        name VARCHAR(64) NOT NULL,
        chain_code VARCHAR(32) NOT NULL,
        node_id VARCHAR(64) NOT NULL,
        protocols TEXT NOT NULL,
        main_symbol VARCHAR(16) NOT NULL,
        status INTEGER NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (chain_code)
    );

CREATE INDEX chain_chain_code_idx ON chain (chain_code);