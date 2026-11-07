-- Add TRON account activation facts for resource-gate preconditions.

CREATE TABLE api_tron_account_activation
(
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    uid                 TEXT        NOT NULL DEFAULT '', -- API 钱包总钱包 UID
    origin_trade_no     TEXT        NOT NULL, -- 原归集/提币交易号
    origin_trade_type   INTEGER     NOT NULL, -- 原交易类型，取 ApiTradeType 数值
    activation_trade_no TEXT        NOT NULL, -- 激活任务号，SDK 本地生成

    chain_code          TEXT        NOT NULL DEFAULT 'tron', -- 第一阶段固定为 tron
    payer_address       TEXT        NOT NULL, -- 激活 TRX 支付地址，通常为出款地址
    target_address      TEXT        NOT NULL, -- 待激活地址
    amount              TEXT        NOT NULL DEFAULT '0.000001', -- 激活转账金额，单位 TRX
    status              INTEGER     NOT NULL DEFAULT 1, -- 1=pending, 2=broadcasted, 3=success, 4=fail

    building_at         TIMESTAMP NULL, -- 链上交易构建占位
    raw_tx              TEXT NULL, -- 原始交易
    tx_hash             TEXT NULL, -- 激活交易哈希
    tx_status           TEXT NULL, -- 链上执行状态视图，如 success / fail / uncertain
    recover_status      INTEGER NULL, -- 恢复/重试视图，1=waiting, 2=retry_build, 3=retry_recover
    last_broadcast_at   TIMESTAMP NULL, -- 最近一次广播时间

    chain_success_at    TIMESTAMP NULL, -- 链上交易成功事实
    chain_failed_at     TIMESTAMP NULL, -- 链上交易失败事实
    account_verified_at TIMESTAMP NULL, -- getaccount/getaccountresource 可查到账户对象

    err_code            TEXT NULL, -- 后端或 SDK 内部错误码原样落库
    err_msg             TEXT NULL, -- 后端或 SDK 内部错误信息原样落库
    next_retry_at       TIMESTAMP NULL, -- 下次重试时间
    retry_count         INTEGER     NOT NULL DEFAULT 0, -- 重试次数

    created_at          TIMESTAMP   NOT NULL,
    updated_at          TIMESTAMP NULL
);

CREATE UNIQUE INDEX api_tron_account_activation_trade_no
    ON api_tron_account_activation (activation_trade_no);
CREATE UNIQUE INDEX api_tron_account_activation_origin_target
    ON api_tron_account_activation (origin_trade_no, origin_trade_type, target_address);
CREATE INDEX api_tron_account_activation_scan
    ON api_tron_account_activation (chain_code, status, next_retry_at);
CREATE INDEX api_tron_account_activation_target
    ON api_tron_account_activation (target_address, chain_code);
