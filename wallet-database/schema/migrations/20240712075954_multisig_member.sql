-- Add migration script here
CREATE TABLE
    multisig_member (
        -- id integer PRIMARY KEY AUTOINCREMENT,
        account_id VARCHAR(64) NOT NULL,
        address VARCHAR(64) NOT NULL,
        pubkey VARCHAR(64) DEFAULT "",
        name VARCHAR(128) NOT NULL,
        confirmed integer NOT NULL,
        is_self integer NOT NULL,
        uid VARCHAR(256) DEFAULT "",
        is_del integer NOT NULL DEFAULT 0,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (account_id, address)
    );