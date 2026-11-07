-- A replacement is a competition between immutable signed identities, not cancellation.
CREATE TABLE api_collect_replacement (
    new_trade_no TEXT PRIMARY KEY NOT NULL,
    old_trade_no TEXT NOT NULL,
    chain_code TEXT NOT NULL,
    from_addr TEXT NOT NULL,
    nonce INTEGER NOT NULL CHECK(nonce >= 0),
    old_hash TEXT NOT NULL,
    old_raw TEXT NOT NULL,
    legacy_counter INTEGER NOT NULL CHECK(legacy_counter >= nonce),
    new_hash TEXT,
    new_raw TEXT,
    winner_hash TEXT,
    abandoned_at TEXT,
    execution_success INTEGER,
    actual_transaction_fee TEXT,
    actual_resource_consume TEXT,
    block_hash TEXT,
    block_number INTEGER,
    transaction_time TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX api_collect_replacement_slot ON api_collect_replacement(chain_code, from_addr, nonce) WHERE abandoned_at IS NULL;
CREATE UNIQUE INDEX api_collect_replacement_old ON api_collect_replacement(old_trade_no) WHERE abandoned_at IS NULL;
-- Other legacy rebuild paths must not erase a linked signed identity.
CREATE TRIGGER api_collect_replacement_identity_guard
BEFORE UPDATE OF raw_tx,tx_hash,nonce,chain_code,from_addr ON api_collect
WHEN EXISTS(SELECT 1 FROM api_collect_replacement r WHERE r.old_trade_no=OLD.trade_no OR (r.new_trade_no=OLD.trade_no AND r.new_hash IS NOT NULL))
 AND (NEW.raw_tx IS NOT OLD.raw_tx OR NEW.tx_hash IS NOT OLD.tx_hash OR NEW.nonce IS NOT OLD.nonce OR NEW.chain_code IS NOT OLD.chain_code OR NEW.from_addr IS NOT OLD.from_addr)
BEGIN
 SELECT RAISE(ABORT,'linked replacement signed identity is immutable');
END;
-- Address-scoped fence: legacy counter-first allocation cannot publish new
-- reservations while this explicitly authorized recovery owns the address.
-- Current signed-first writers publish raw/hash before raising the counter.
CREATE TRIGGER api_collect_replacement_counter_fence
BEFORE UPDATE OF nonce ON api_nonce
WHEN NEW.nonce > OLD.nonce
 AND EXISTS(SELECT 1 FROM api_collect_replacement r WHERE r.chain_code=NEW.chain_code AND r.from_addr=lower(trim(NEW.from_addr)) AND r.abandoned_at IS NULL AND r.winner_hash IS NULL AND NEW.nonce>r.nonce)
 AND NOT EXISTS(SELECT 1 FROM api_collect c WHERE c.chain_code=NEW.chain_code AND lower(trim(c.from_addr))=lower(trim(NEW.from_addr)) AND c.nonce=NEW.nonce AND (c.raw_tx IS NOT NULL OR c.tx_hash IS NOT NULL))
 AND NOT EXISTS(SELECT 1 FROM api_fee f WHERE f.chain_code=NEW.chain_code AND lower(trim(f.from_addr))=lower(trim(NEW.from_addr)) AND f.nonce=NEW.nonce AND (f.raw_tx IS NOT NULL OR f.tx_hash IS NOT NULL))
 AND NOT EXISTS(SELECT 1 FROM api_withdraws w WHERE w.chain_code=NEW.chain_code AND lower(trim(w.from_addr))=lower(trim(NEW.from_addr)) AND w.nonce=NEW.nonce AND (w.raw_tx IS NOT NULL OR w.tx_hash IS NOT NULL))
BEGIN SELECT RAISE(ABORT,'scoped replacement requires signed-first nonce ownership'); END;
CREATE TRIGGER api_collect_replacement_counter_insert_fence
BEFORE INSERT ON api_nonce
WHEN EXISTS(SELECT 1 FROM api_collect_replacement r WHERE r.chain_code=NEW.chain_code AND r.from_addr=lower(trim(NEW.from_addr)) AND r.abandoned_at IS NULL AND r.winner_hash IS NULL AND NEW.nonce>r.nonce)
 AND NOT EXISTS(SELECT 1 FROM api_collect c WHERE c.chain_code=NEW.chain_code AND lower(trim(c.from_addr))=lower(trim(NEW.from_addr)) AND c.nonce=NEW.nonce AND (c.raw_tx IS NOT NULL OR c.tx_hash IS NOT NULL))
 AND NOT EXISTS(SELECT 1 FROM api_fee f WHERE f.chain_code=NEW.chain_code AND lower(trim(f.from_addr))=lower(trim(NEW.from_addr)) AND f.nonce=NEW.nonce AND (f.raw_tx IS NOT NULL OR f.tx_hash IS NOT NULL))
 AND NOT EXISTS(SELECT 1 FROM api_withdraws w WHERE w.chain_code=NEW.chain_code AND lower(trim(w.from_addr))=lower(trim(NEW.from_addr)) AND w.nonce=NEW.nonce AND (w.raw_tx IS NOT NULL OR w.tx_hash IS NOT NULL))
BEGIN SELECT RAISE(ABORT,'scoped replacement requires signed-first nonce ownership'); END;
