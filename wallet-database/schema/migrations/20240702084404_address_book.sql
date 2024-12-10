-- Add migration script here
CREATE TABLE
    address_book (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name VARCHAR(64) NOT NULL,
        address VARCHAR(128) NOT NULL,
        chain_code VARCHAR(32) NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP
    );