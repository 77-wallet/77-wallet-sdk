-- Add post-order fields to original API wallet transactions.

ALTER TABLE api_collect ADD COLUMN resource_check_at TIMESTAMP NULL; -- 最近一次资源决策/检查时间，仅用于观测、排障和恢复扫描节流，不作为 CanBuild 放行条件
ALTER TABLE api_collect ADD COLUMN resource_gate_released_at TIMESTAMP NULL; -- 资源闸门释放时间；TRON 原交易进入 BuildTx 的资源前置事实
ALTER TABLE api_collect ADD COLUMN resource_gate_result INTEGER NULL; -- 资源闸门释放结果（内部枚举）
ALTER TABLE api_collect ADD COLUMN resource_block_reason INTEGER NULL; -- 当前资源阻塞原因（内部枚举）
ALTER TABLE api_collect ADD COLUMN resource_dependency_trade_no TEXT NULL; -- 当前依赖的资源任务号，用于从原交易反查资源代理/回收事实
ALTER TABLE api_collect ADD COLUMN resource_dependency_type INTEGER NULL; -- 当前依赖资源任务类型（内部枚举）

ALTER TABLE api_withdraws ADD COLUMN resource_check_at TIMESTAMP NULL; -- 最近一次资源决策/检查时间，仅用于观测、排障和恢复扫描节流，不作为 CanBuild 放行条件
ALTER TABLE api_withdraws ADD COLUMN resource_gate_released_at TIMESTAMP NULL; -- 资源闸门释放时间；TRON 原交易进入 BuildTx 的资源前置事实
ALTER TABLE api_withdraws ADD COLUMN resource_gate_result INTEGER NULL; -- 资源闸门释放结果（内部枚举）
ALTER TABLE api_withdraws ADD COLUMN resource_block_reason INTEGER NULL; -- 当前资源阻塞原因（内部枚举）
ALTER TABLE api_withdraws ADD COLUMN resource_dependency_trade_no TEXT NULL; -- 当前依赖的资源任务号，用于从原交易反查资源代理/回收事实
ALTER TABLE api_withdraws ADD COLUMN resource_dependency_type INTEGER NULL; -- 当前依赖资源任务类型（内部枚举）

CREATE INDEX api_collect_resource_gate_scan
    ON api_collect (chain_code, resource_gate_released_at, resource_dependency_trade_no);

CREATE INDEX api_withdraws_resource_gate_scan
    ON api_withdraws (chain_code, trade_type, resource_gate_released_at, resource_dependency_trade_no);

ALTER TABLE api_withdraws ADD COLUMN out_order_id TEXT NULL;
ALTER TABLE api_withdraws ADD COLUMN client_id TEXT NULL;
ALTER TABLE api_withdraws ADD COLUMN create_time TEXT NULL;

ALTER TABLE api_withdraws ADD COLUMN estimated_transaction_fee TEXT NULL;
ALTER TABLE api_withdraws ADD COLUMN estimated_resource_consume TEXT NULL;
ALTER TABLE api_withdraws ADD COLUMN fee_estimated_at TIMESTAMP NULL;
