-- Add migration script here
ALTER TABLE config ADD COLUMN types INT DEFAULT 0 NOT NULL;