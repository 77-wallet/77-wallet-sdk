-- Add migration script here
CREATE TABLE permission_user
(
    id integer PRIMARY KEY AUTOINCREMENT,
    grantor_addr VARCHAR(128) NOT NULL,
    address VARCHAR(64) NOT NULL,
    permission_id VARCHAR(64) NOT NULL,
    is_self INTEGER NOT NULL,
    weight INTEGER NOT null,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);