-- Add migration script here
ALTER TABLE chain ADD COLUMN node_bind_type INTEGER NOT NULL DEFAULT 0;
