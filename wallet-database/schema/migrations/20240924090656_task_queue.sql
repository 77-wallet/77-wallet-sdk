-- Add migration script here
CREATE TABLE
    task_queue (
        id VARCHAR(64) NOT NULL,
        task_name VARCHAR(64) NOT NULL,
        request_body TEXT DEFAULT '' NOT NULL,
        type INTEGER NOT NULL,
        status INTEGER NOT NULL DEFAULT 0,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (id)
    );