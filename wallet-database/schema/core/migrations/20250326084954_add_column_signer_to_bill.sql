-- Add migration script here
ALTER TABLE bill ADD COLUMN signer VARCHAR DEFAULT "" NOT NULL;