-- Add migration script here
CREATE TABLE
    assets (
        name VARCHAR(64) NOT NULL,
        symbol VARCHAR(64) NOT NULL,
        decimals INTEGER NOT NULL,
        address VARCHAR(28) NOT NULL,
        token_address VARCHAR(128) NULL,
        protocol VARCHAR(20) NULL,
        chain_code VARCHAR(32) NOT NULL,
        status INTEGER NOT NULL,
        is_multisig INTEGER DEFAULT 0 NOT NULL,
        balance VARCHAR(256) NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (symbol, address, chain_code)
    );

CREATE INDEX assets_name_address_chain_code_idx ON assets (symbol, address, chain_code);