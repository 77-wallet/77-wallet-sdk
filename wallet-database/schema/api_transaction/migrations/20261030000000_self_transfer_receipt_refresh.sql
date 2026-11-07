-- A terminal chain fact and its pending frontend invalidation are committed together.
ALTER TABLE api_withdraws ADD COLUMN receipt_refresh_pending INTEGER NOT NULL DEFAULT 0 CHECK (receipt_refresh_pending IN (0, 1));
CREATE INDEX idx_self_transfer_receipt_refresh ON api_withdraws(id) WHERE receipt_refresh_pending = 1;
CREATE INDEX idx_signed_self_transfer_identity ON api_withdraws(chain_code, lower(tx_hash), lower(from_addr)) WHERE trade_type = 4 AND raw_tx IS NOT NULL AND raw_tx != '';
