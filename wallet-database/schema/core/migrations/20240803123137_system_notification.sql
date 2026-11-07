-- Add migration script here
CREATE TABLE
    system_notification (
        id VARCHAR(64) PRIMARY KEY,
        type TEXT NOT NULL,
        key VARCHAR(64) NUll,
        value VARCHAR(64) NUll,
        content TEXT NOT NULL,
        status INTEGER,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP
    );

CREATE INDEX system_notification_id_idx ON system_notification (id);