-- Add migration script here
CREATE TABLE
    bill (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash VARCHAR(128) NOT NULL, --交易哈希
        chain_code VARCHAR(32) NOT NULL, --链码
        symbol VARCHAR(32) NOT NULL, --币种符号
        transfer_type INTEGER NOT NULL, --交易方式 0转入 1转出
        tx_kind INTEGER NOT NULL, --交易类型 1:普通交易，2:部署多签账号 3:服务费
        owner VARCHAR(128) NOT NULL, --订单归属
        from_addr VARCHAR(128) NOT NULL, --发起方
        to_addr VARCHAR(128) NOT NULL, --接收方
        token VARCHAR(128), --合约地址
        value VARCHAR(256) NOT NULL, --交易额
        transaction_fee VARCHAR(256) NOT NULL, --手续费
        resource_consume VARCHAR(256) DEFAULT "0",
        transaction_time TIMESTAMP NULL, --交易时间
        status INTEGER NOT NULL, --交易状态 1-pending 2-成功 3-失败
        is_multisig INTEGER DEFAULT 0 NOT NULL, --是否多签
        block_height VARCHAR(32) NULL, --块高
        queue_id VARCHAR(64) DEFAULT "", --队列id
        notes TEXT NOT NULL, --备注
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP,
        UNIQUE (hash, transfer_type, owner)
    );

CREATE INDEX bill_hash_idx ON bill (hash);