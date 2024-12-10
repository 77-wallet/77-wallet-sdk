-- Add migration script here
CREATE TABLE
    announcement (
        id VARCHAR(64) NOT NULL,
        title VARCHAR(64) NOT NULL,
        content TEXT NOT NULL,
        language VARCHAR(64) DEFAULT 'ENGLISH' NOT NULL,
        remark TEXT,
        status INTEGER NOT NULL,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (id)
    );