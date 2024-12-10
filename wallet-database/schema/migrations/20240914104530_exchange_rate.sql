-- Add migration script here
CREATE TABLE
    exchange_rate (
        name VARCHAR(64) NOT NULL,
        rate REAL NOT NULL,
        target_currency VARCHAR(64) NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (target_currency)
    );