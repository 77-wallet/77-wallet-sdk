-- Add migration script here
CREATE TABLE
    multisig_signatures (
        id integer PRIMARY KEY AUTOINCREMENT,
        queue_id VARCHAR(64) NOT NULL,
        address VARCHAR(64),
        status INTEGER,
        signature VARCHAR(128),
        is_del integer NOT NULL DEFAULT 0,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP
    );