# EVM 转账附加费用：Core / SDK 职责边界

## 目标与约束

将 2.2 分支中 Base、Scroll 的链专属 L1 数据费估算下沉至 `wallet-chain-interact`，避免 `wallet-api` 直接构造链协议 RPC。标准钱包主币和 ERC-20 普通转账的费用请求、后端 `findGasTracker` 协议、前端三档 JSON 与 `feeSetting` 语义均保持不变。Arbitrum、Polygon、Avalanche 的费用行为不在本次迁移中改变；不扩展 Swap、任意合约调用或 API Wallet。

用户审核前不提交、不合并、不推送。SDK 2.2 工作树现有未提交 Base 改动要原样保留，迁移在此基础上进行。Core 使用独立的 2.2 功能分支，不在硬件钱包分支开发。

## 方案比较

1. 保持全部逻辑在 SDK：改动最小，但 Core 无法复用链协议能力，且附加费需重新选择 RPC 节点。
2. 整个 `evm_fee` 搬进 Core：会让 Core 依赖 SDK 的 Context、后端 tracker、币价和前端 JSON，打破依赖方向。
3. **采用：只下沉链专属 RPC 能力。** Core 使用 `EthChain` 已持有的 `Provider`，按链调用 Scroll RPC 或 Base GasPriceOracle，返回 wei；SDK 保留报价编排和展示转换。此方案既复用同一节点，也保持 Core 不依赖业务层。

## 模块与接口

- Core `wallet-chain-interact/src/eth/` 增加聚焦的附加费用模块。对外提供 `EthChain::estimate_additional_fee(tx: TransactionRequest) -> crate::Result<Option<U256>>`。`None` 表示该链不需额外费用；`Some(wei)` 表示附加的主币费用。支持的链由一个集中策略选择；Base 与 Scroll 的实际请求/解析分别放入独立文件。
- Core `Provider` 为该模块提供对已有 `RpcClient` 的受控调用，复用构造 `EthChain` 时选中的端点、鉴权头和超时，不暴露原始 client 给 SDK。Core 错误使用现有 `crate::Error` 类型；失败及无效/零值不得被转换成 `None` 或零费用。
- SDK `wallet-api/.../evm_fee/mod.rs` 保留余额校验、`findGasTracker`、`TransferOpt`、执行 gas、币价与 `FeeDetails` 三档组合。它只从 Core 获取 `Option<U256>` 并加到展示总费用；`feeSetting` 仍只表示签名所需的执行层 gas 参数。
- SDK 移除 Base/Scroll 直连 RPC 文件及费用模块里再次调用 `get_chain_node_with_ctx` 的路径。前端请求和返回 JSON 不变。

## 数据流与错误语义

SDK 根据转账请求构造与 `eth_estimateGas` 一致的 `TransactionRequest`，调用 `EthChain::estimate_additional_fee`。Core 用同一个 Provider 估算链专属附加费；SDK 再取得执行 gas 和后端 tracker，生成三档费用。无附加费的链继续原有执行费流程。Base/Scroll 附加费 RPC 错误、返回格式不对或返回零时，整个报价失败，防止向用户展示低估金额。

本次迁移不重新定义 Base `getL1FeeUpperBound` 的保守交易大小，也不重新定义 Scroll `scroll_estimateL1DataFee` 的交易对象。现有算法与测试作为迁移基线；若真实节点验证发现算法本身不正确，另起问题修正，不在搬迁中暗改费用口径。

## 分批实施与验证

改动横跨 Core 与 SDK 两仓，按仓库变更规模规则分批：

1. Core 批次：先增加离线 RPC/错误测试，再迁入 Base 与 Scroll 的节点调用；仅验证 `wallet-chain-interact` 的受影响测试、检查和格式。
2. SDK 批次：更新 Core 修订依赖，改用 Core 接口，删除旧直连实现；覆盖主币/ERC-20、三档总额、非附加费链不调用附加 RPC，以及错误传播。仅验证 `wallet-api` 受影响测试、检查和格式。
3. 最后复核前端 JSON 契约、Git 差异、相关断言矩阵；真实节点只做只读验证，结果与离线测试区分报告。

每批以测试通过、行为边界清晰为停止条件。SDK 对 Core 的修订锁定在 Core 批次通过测试后的具体提交；提交前先让用户审核，不能直接提交。
