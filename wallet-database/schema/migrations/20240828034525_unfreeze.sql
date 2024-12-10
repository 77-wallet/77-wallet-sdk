-- Add migration script here
CREATE TABLE
    unfreeze (
        id VARCHAR(64) NOT NULL,
        tx_hash VARCHAR(64) NOT NULL,
        owner_address VARCHAR(32) NOT NULL,
        resource_type VARCHAR(32) NOT NULL,
        amount VARCHAR(256) NOT NULL,
        freeze_time INTEGER NOT NULL,
        created_at TIMESTAMP NOT NULL,
        PRIMARY KEY (id)
    );
