-- Add migration script here
CREATE TABLE api_fee
(
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    uid                   TEXT        NOT NULL DEFAULT '',  -- 总钱包
    name                  VARCHAR(64)             NOT NULL, -- 总钱包名称
    from_addr             VARCHAR(64)             NOT NULL,
    to_addr               VARCHAR(64)             NOT NULL,
    value                 VARCHAR(64)             NOT NULL,
    validate              VARCHAR(64)             NOT NULL,
    chain_code            VARCHAR(64)             NOT NULL,
    token_addr            VARCHAR(128) NULL,
    symbol                VARCHAR(128) DEFAULT "" NOT NULL,
    trade_no              VARCHAR(32)             NOT NULL,
    trade_type            INTEGER                 NOT NULL,
    status                INTEGER      DEFAULT 0  NOT NULL, -- UI/人类可读状态，不参与执行逻辑
    nonce                 INTEGER      DEFAULT 0  NOT NULL, -- nonce
    tx_hash               VARCHAR(64) NULL,                 -- 交易哈希
    raw_tx                TEXT NULL,                        -- 原始交易
    resource_consume      VARCHAR(256) DEFAULT "0" NOT NULL, -- 资源消耗
    transaction_fee       VARCHAR(256) DEFAULT "" NOT NULL, -- 手续费
    transaction_time      TIMESTAMP NULL,                   -- 交易时间
    block_height          VARCHAR(32) NULL,                 -- 块高
    notes                 TEXT NULL,                        -- 备注
    post_tx_count         INTEGER      DEFAULT 0  NOT NULL, -- 已发送交易次数
    post_confirm_tx_count INTEGER      DEFAULT 0  NOT NULL, -- 已确认交易次数
    err_code              INTEGER NULL, -- 错误码
    err_msg               TEXT NULL,                        -- 错误信息
    
    -- ===== Tx ACK（交易 ACK 事实）=====
    tx_ack_sent_at        TIMESTAMP NULL,                   -- 确认已接收并持久化该交易
    
    -- ===== Build / Broadcast Execution Facts =====
    building_at           TIMESTAMP NULL,                   -- BuildTx 执行占位
    last_broadcast_at     TIMESTAMP NULL,                   -- 最近一次 Broadcast 执行占位
    broadcast_uncertain_since_at TIMESTAMP NULL,            -- EVM 广播不确定态开始时间
    broadcast_uncertain_retry_count INTEGER DEFAULT 0 NOT NULL, -- EVM 广播不确定态重试次数
    broadcast_uncertain_last_checked_at TIMESTAMP NULL,     -- EVM 广播不确定态最近检查时间
    broadcast_uncertain_reconciled_at TIMESTAMP NULL,       -- EVM 广播不确定态调和完成时间
    broadcast_uncertain_rebroadcast_count INTEGER DEFAULT 0 NOT NULL, -- EVM 不确定态重播次数
    
    -- ===== Tx Result ACK（结果确认事实）=====
    tx_res_ack_sent_at    TIMESTAMP NULL,                   -- 确认已将交易结果可靠告知后端
    tx_res_received_at    TIMESTAMP NULL,                   -- 已接收并持久化 SER TxRes push 事实
    
    -- ===== Tx Exec Receipt Upload（交易执行回执上传事实）=====
    tx_exec_receipt_uploaded_at TIMESTAMP NULL,             -- 已上传交易执行回执

    
    -- ===== Terminal Fact =====
    finished_at           TIMESTAMP NULL,                   -- 链上终态事实
    
    created_at            TIMESTAMP               NOT NULL,
    updated_at            TIMESTAMP NULL
);

CREATE INDEX api_fee_from ON api_fee (uid, from_addr);
CREATE UNIQUE INDEX api_fee_trade_no ON api_fee (trade_no);
-- 此索引用于加速查询未发送 ACK 的fee记录，提高幂等性检查效率
CREATE INDEX api_fee_ack_times ON api_fee (tx_ack_sent_at, tx_res_ack_sent_at);
