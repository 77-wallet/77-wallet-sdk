-- Add password_proof field to device table
ALTER TABLE device
ADD COLUMN password_proof TEXT NULL;
