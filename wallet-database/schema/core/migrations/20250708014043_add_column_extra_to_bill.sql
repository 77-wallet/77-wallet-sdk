-- Add migration script here
ALTER TABLE bill ADD COLUMN extra TEXT DEFAULT "" NOT NULL;