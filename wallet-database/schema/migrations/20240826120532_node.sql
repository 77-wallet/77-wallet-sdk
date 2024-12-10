-- Add migration script here
CREATE TABLE
    node (
        node_id VARCHAR(64) NOT NULL,
        name VARCHAR(64) NOT NULL,
        chain_code VARCHAR(32) NOT NULL,
        rpc_url VARCHAR(256) NOT NULL,
        ws_url VARCHAR(256) NOT NULL,
        http_url VARCHAR(256) NOT NULL,
        status INTEGER NOT NULL,
        network VARCHAR(16) NUll DEFAULT "mainnet",
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        PRIMARY KEY (node_id)
    );

CREATE INDEX node_id_idx ON node (node_id);