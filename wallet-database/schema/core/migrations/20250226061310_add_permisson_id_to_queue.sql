-- Add migration script here
ALTER TABLE multisig_queue ADD COLUMN permission_id VARCHAR NOT NULL DEFAULT '';