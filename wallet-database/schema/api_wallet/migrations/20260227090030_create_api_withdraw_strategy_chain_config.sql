CREATE TABLE IF NOT EXISTS api_withdraw_strategy_chain_config (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    strategy_id INTEGER NOT NULL,
    chain_code VARCHAR(32) NOT NULL,
    chain_address_type VARCHAR(32),
    normal_idx INTEGER,
    normal_address VARCHAR(128) NOT NULL,
    risk_idx INTEGER,
    risk_address VARCHAR(128) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP,
    UNIQUE(strategy_id, chain_code),
    FOREIGN KEY(strategy_id) REFERENCES api_withdraw_strategy(id) ON DELETE CASCADE
);
