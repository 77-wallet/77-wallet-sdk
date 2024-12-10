-- Add migration script here
CREATE TABLE
    wallet (
        name VARCHAR(64) NOT NULL,
        uid VARCHAR(64) NOT NULL,
        address VARCHAR(64) NOT NULL,
        status INTEGER NOT NULL,
        is_init INTEGER NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (address)
    );

CREATE INDEX wallet_address_idx ON wallet (address);