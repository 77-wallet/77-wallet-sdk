-- Add migration script here
CREATE TABLE
    delegate (
        id VARCHAR(64) NOT NULL,
        tx_hash VARCHAR(64) NOT NULL,
        owner_address VARCHAR(32) NOT NULL,
        receiver_address VARCHAR(32) NOT NULL,
        resource_type VARCHAR(32) NOT NULL,
        amount VARCHAR(256) NOT NULL,
        status INTEGER NOT NULL,
        lock INTEGER NOT NULL,
        lock_period INTEGER NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (id)
    );