-- Add migration script here
CREATE TABLE
    coin (
        name VARCHAR(64) NULL,
        symbol VARCHAR(64) NOT NULL,
        chain_code VARCHAR(64) NOT NULL,
        token_address VARCHAR(128) NULL,
        protocol VARCHAR(20) NULL,
        price VARCHAR(256) NULL,
        decimals INTEGER NOT NULL,
        is_default INTEGER NOT NULL DEFAULT 0,
        is_popular INTEGER NOT NULL DEFAULT 0,
        status INTEGER NOT NULL DEFAULT 1,
        is_del INTEGER NOT NULL DEFAULT 0,
        is_custom INTEGER NOT NULL DEFAULT 0,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (symbol, chain_code, token_address)
    );

CREATE INDEX coin_symbol_chain_code_token_address_idx ON assets (symbol, chain_code, token_address);