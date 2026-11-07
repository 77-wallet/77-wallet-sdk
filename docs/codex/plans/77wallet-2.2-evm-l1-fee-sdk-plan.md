# 77Wallet 2.2 EVM 链费用估算方案（SDK）

## 范围与目标

范围是 2.2 新增 EVM 链：Polygon、Avalanche C-Chain、Arbitrum、Base、Scroll；Fantom 已从需求排除。

目标是在不改变前端费用请求参数、后端 `token/findGasTracker` 协议和现有费用 JSON 解析的前提下，让 SDK 对普通主币转账及 ERC-20 转账计算足够完整的网络费。实现应仅在确有协议额外费用的链上增加 L1 数据费；不要把所有 EVM 链都当作 L2。

## 代码现状

- 前端调用 `transaction_fee`，输入映射到 `BaseTransferReq`：`from`、`to`、`value`、`chain_code`、`symbol`、token 地址等。
- `TransactionService::transaction_fee` 从 coin 配置补齐 decimals/token，再进入 `TransactionAdapter::estimate_fee_with_ctx`。
- EVM 分支用请求构造 `TransferOpt`，调用 `chain.estimate_gas(params)` 得到 gas limit；`findGasTracker`（SDK `gas_oracle`）提供 `safe/propose/fast` gas 价格档位。
- `FeeDetails<EthereumFeeDetails>` 将执行费组织成现有 JSON：`default`、`data[]`，每项有 `type`、`estimateFee`、`maxFee`、`feeSetting`。其中 `feeSetting` 是实际发交易用的 EIP-1559 参数，不是通用总费用容器。
- 当前 `estimateFee` 和 `maxFee` 只按 gas limit 与执行层 gas price 计算。`GasOracle` 没有 L1 数据费用字段；SDK 也没有统一的 EVM L1 费用模型。
- 普通转账的 calldata 可由 SDK 从现有字段构造：原生币转账使用空 data；ERC-20 转账由 token transfer 编码生成。无需为本次普通转账估算让前端新增 calldata 参数。任意合约调用/Swap 不在本计划范围内。

## 按链处理策略

| 链 | 初步费用模型 | SDK 计划 | 当前结论/验证点 |
|---|---|---|---|
| Polygon | EVM 执行费 | 沿用 `eth_estimateGas` + `findGasTracker` | 暂无本需求范围内的额外 L1 数据费；确认实际配置节点返回字段兼容现有 GasOracle。 |
| Avalanche C-Chain | EVM 执行费 | 沿用现有 EVM 流程 | 暂无本需求范围内的额外 L1 数据费；核对 gas oracle、最小 tip 与单位。 |
| Arbitrum | L2 执行费用及 parent-chain 数据成本由 Nitro 的 gas 估算机制组合 | 探测并集成 `NodeInterface.gasEstimateComponents`，基于同一交易的 from/to/data/value 解析组件；以协议返回的 total gas estimate/组件语义为准，不能在已经包含 L1 buffer 的 gasLimit 上再次简单加一笔 L1 费 | 官方 SDK 示例通过预编译 NodeInterface 取得 L1 组件。需按当前 Arbitrum 官方 ABI/返回字段确认 SDK 使用正确组件，并用 RPC 结果交叉验证，防重复计费。 |
| Base | OP Stack 风格 L2 执行费 + L1 数据发布费（协议升级还可能影响费用组成） | `eth_estimateGas` 处理执行 gas；另调用 Base 当前支持的官方 L1 fee oracle/估算机制，以构造交易的数据估算 L1 费；分别维护组件并求展示总额 | 需在编码前核实当前 Base 官方 oracle 地址、调用参数（交易编码格式）、当前 fork 费用口径、单位。之前的节点方法探测仅证明可响应，不作为算法正确证据。 |
| Scroll | L2 execution fee + L1 data fee | `eth_estimateGas` 估执行 gas；通过 Scroll RPC 的 `scroll_estimateL1DataFee`（若部署节点支持）或官方 L1GasPriceOracle `getL1Fee` 对应交易数据求 L1 费 | Scroll 官方文档说明总费为两部分；特别核实所用入口要求的 data 是调用 calldata 还是完整签名 RLP 交易。官方 `getL1Fee` 文档描述输入为签名后的 RLP 交易，不能未经核对就传未签名 calldata。 |

## 建议实现结构

1. 保持外部接口不变：前端 `transaction_fee` 参数、后端 `findGasTracker` 和 SDK 对前端输出 JSON 的字段结构不变。
2. 在 SDK EVM 费用估算内部抽象两个部分：
   - `execution_fee`: 每档 gas limit × 对应 execution gas price。
   - `l1_data_fee`: 对 Arbitrum/Base/Scroll 按各链官方算法估算；Polygon/Avalanche 默认为 0。
3. 每档生成费用时，`estimateFee` 与 `maxFee` 应按产品定义表达总费用（执行费 + L1 数据费/相应安全余量），而 `feeSetting` 仍只保留链上交易真正需要的 `gasLimit/baseFee/priorityFee/maxFeePerGas`。不可把 L1 一次性金额折算进 `maxFeePerGas`。
4. 先实现普通转账报价，不扩张至 Swap/任意 calldata。提取或保留构造后的交易描述（from/to/value/data），避免费用估算和最终签名交易使用不同 calldata。
5. L1 oracle 不可用或返回无效值时，不能静默把 L1 费用当 0 并显示为完整报价。明确失败/降级策略：优先失败并提示无法估算；只有产品明确接受“仅显示执行费”时才提供显式不完整报价标识。
6. 费用报价与后续余额校验、max fee 预留、实际交易回执记账分开审视。报价合计有 L1 费，不代表交易的 EIP-1559 `feeSetting` 可承载它；实际消耗应从链的回执/协议数据取值，避免用报价覆盖实际费用。

## 验证步骤

### 离线单元测试

- 原生币与 ERC-20 转账分别构造预期交易数据；验证两类调用生成的 data 被交给对应 L1 oracle。
- 对 Arbitrum 返回组件验证 SDK 使用正确的 L2/L1/total 口径，不重复加总。
- 对 Base/Scroll 验证 hex/ABI/RLP 编码、wei 单位转换与总额加法；覆盖空 data、非空 ERC-20 calldata、溢出/无效 hex/节点错误。
- 验证旧 EVM 链 (Ethereum、BNB 等) 输出 JSON 和 feeSetting 行为不变。
- 验证 L1 费用估算失败时按选定策略处理，不能意外报价为零。

### 真实节点只读探测

- 使用已有鉴权逻辑访问配置的 dev/test RPC；仅发 `eth_estimateGas`、`eth_call` 或链专属只读估算请求。
- 使用固定 from/to/value 和 SDK 构造的原生币/代币 transfer data；不签名、不广播、不消耗测试币。
- 记录每链 RPC 方法、请求参数（遮蔽 token）、原始结果、单位、SDK 换算结果。测试端点连通性与数值正确性分开记结论。

### 上线前费用精度验证

- 在测试网有可用测试币时，选取已完成的主币及 ERC-20 转账，对比 SDK 报价中的总费用与链上回执实际费用。
- 该对比是精度验收，不是开始实现的前置条件；不得为了测试向真实地址广播真实资产交易。

## 待逐链确认的事实

- Arbitrum NodeInterface ABI 的准确返回项、哪个字段代表 total gas estimate，以及它与现有 `gasLimit × gasPrice` 的关系。
- Base 当前网络版本对应的 L1 fee oracle/估算 RPC、地址、输入编码与完整计算口径。
- Scroll 节点扩展方法与官方 `getL1Fee` 的参数格式是否相同，以及未签名报价阶段应采用哪条受支持路径。
- `maxFee` 字段展示 L1 费用的安全余量规则；产品对估算服务失败时的 UI/错误策略。

## 官方参考

- Scroll: [Transaction Fees on Scroll](https://docs.scroll.io/en/developers/transaction-fees-on-scroll/)
- Arbitrum: [Official OffchainLabs gas-estimation example](https://github.com/OffchainLabs/arbitrum-tutorials/blob/master/packages/gas-estimation/scripts/exec.ts)
- Base: [Base protocol derivation](https://docs.base.org/base-chain/specs/protocol/consensus/derivation)
- Base JSON-RPC: [Base Ethereum JSON-RPC API](https://docs.base.org/base-chain/api-reference/ethereum-json-rpc-api)
