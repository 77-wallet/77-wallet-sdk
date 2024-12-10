-- Add migration script here
CREATE TABLE
    config (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        key VARCHAR(64) NOT NULL UNIQUE,
        value VARCHAR(255) NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP
    );