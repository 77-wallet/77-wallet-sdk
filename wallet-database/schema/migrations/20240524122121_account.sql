-- Add migration script here
CREATE TABLE
    account (
        account_id INTEGER NOT NULL,
        name VARCHAR(64) NOT NULL,
        address VARCHAR(128) NOT NULL,
        pubkey VARCHAR(128) NULL,
        address_type VARCHAR(64) DEFAULT "",
        wallet_address VARCHAR(128) NOT NULL,
        derivation_path VARCHAR(32) NULL,
        chain_code VARCHAR(32) NOT NULL,
        status INTEGER NOT NULL,
        is_init INTEGER NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (address, chain_code)
    );

CREATE INDEX account_address_chain_code_id_idx ON account (address, chain_code);