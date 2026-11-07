-- Add API wallet resource stake/unstake operation facts.

CREATE TABLE api_resource_operation
(
    id                           INTEGER PRIMARY KEY AUTOINCREMENT,
    uid                          TEXT        NOT NULL DEFAULT '', -- API 钱包总钱包 UID
    task_source                  INTEGER     NOT NULL, -- 任务来源：1=backend 后端下发；2=client 客户端主动发起
    operation_type               INTEGER     NOT NULL, -- 资源操作：1=stake 质押；2=unstake 解质押
    resource_trade_no            TEXT        NOT NULL, -- 资源操作任务号；backend 使用后端 tradeNo，client 使用 SDK 本地 UUID
    chain_code                   TEXT        NOT NULL DEFAULT 'tron', -- 当前资源操作链，第一阶段固定为 tron
    owner_address                TEXT        NOT NULL, -- 执行质押/解质押的钱包地址
    receiver_address             TEXT NULL, -- 资源接收方；质押/解质押通常为空，保留给扩展场景
    resource_type                INTEGER     NOT NULL DEFAULT 1, -- 资源类型：0=bandwidth；1=energy
    amount                       TEXT        NOT NULL DEFAULT '0', -- 质押/解质押数量，字符串保存避免精度/单位转换损失
    status                       INTEGER     NOT NULL DEFAULT 1, -- 任务阶段视图：1=pending；执行推进必须以事实字段为准

    task_ack_sent_at             TIMESTAMP NULL, -- backend 资源操作任务接收 ACK 已发送时间；client 不使用
    building_at                  TIMESTAMP NULL, -- 链上交易构建占位，防止重复构建
    raw_tx                       TEXT NULL, -- 已构建的原始交易内容；broadcast/recover 必须基于该事实
    tx_hash                      TEXT NULL, -- 资源操作链上交易哈希
    transaction_fee              TEXT NULL, -- 构建阶段预估/计算出的交易手续费
    last_broadcast_at            TIMESTAMP NULL, -- 最近一次成功广播时间；不代表链上最终确认
    broadcast_uncertain_since_at TIMESTAMP NULL, -- 广播不确定状态起始时间
    broadcast_uncertain_retry_count INTEGER NOT NULL DEFAULT 0, -- 广播不确定状态重试次数
    broadcast_uncertain_last_checked_at TIMESTAMP NULL, -- 广播不确定状态最近检查时间
    broadcast_uncertain_reconciled_at TIMESTAMP NULL, -- 广播不确定状态已收敛时间
    transaction_time             TIMESTAMP NULL, -- 链上最终确认时间；必须来自链上查询结果
    tx_status                    TEXT NULL, -- 链上执行状态视图，如 success / fail / uncertain
    tx_exec_receipt_uploaded_at  TIMESTAMP NULL, -- 链上执行回执已上传时间

    result_status                TEXT NULL, -- 后端最终结果或本地最终结果状态
    result_received_at           TIMESTAMP NULL, -- 已接收并持久化最终结果时间
    result_ack_sent_at           TIMESTAMP NULL, -- backend 最终结果 ACK 已发送时间；client 不使用
    result_payload               TEXT NULL, -- 原始最终结果 payload，便于排障和兼容字段回放

    fail_type                    INTEGER NULL, -- 后端 failType 原样落库，不直接面向用户展示
    err_code                     TEXT NULL, -- 后端或 SDK 内部错误码原样落库
    err_msg                      TEXT NULL, -- 后端或 SDK 内部错误信息原样落库

    recover_status               INTEGER NULL, -- 恢复状态枚举：1=recover_waiting, 2=retry_build, 3=retry_recover
    next_retry_at                TIMESTAMP NULL, -- 下次恢复/重试时间
    retry_count                  INTEGER     NOT NULL DEFAULT 0, -- 恢复/重试次数

    created_at                   TIMESTAMP   NOT NULL, -- 创建时间
    updated_at                   TIMESTAMP NULL -- 更新时间
);

CREATE UNIQUE INDEX api_resource_operation_trade_no
    ON api_resource_operation (resource_trade_no);
CREATE INDEX api_resource_operation_scan
    ON api_resource_operation (task_source, operation_type, id);
CREATE INDEX api_resource_operation_owner
    ON api_resource_operation (owner_address, resource_type);
CREATE INDEX api_resource_operation_hash_owner
    ON api_resource_operation (tx_hash, owner_address);
