# 2.2 EVM 普通转账费用模块设计

## 目标与范围

本轮重构 2.2 分支上尚未提交的 Scroll 标准钱包转账费用代码，让五条新增链的费用差异有一个集中扩展点。只完成 Scroll 主币和 ERC-20 普通转账的 L1 数据费；不在本轮实现 Arbitrum、Base 的附加费用，也不扩展 Swap、任意合约调用或 API Wallet。前端请求、后端 gas tracker 接口和现有三档费用 JSON 均保持不变。Fantom 已从本次需求排除。

## 方案选择

- 继续在 `transaction_adapter.rs` 增加链判断：改动少，但每条链都会把通用报价和链专属 RPC 混在一起；不采用。
- 每条链建立完整的报价 trait 实现：隔离充分，但目前只有 Scroll 需要新逻辑，五套实现会复制余额检查、tracker 与 gas 估算；暂不采用。
- **采用集中编排 + 单点策略选择**：通用 EVM 报价只写一次，策略选择集中在 `evm_fee` 模块，真正需要额外费用的链按需增加实现文件。新增链最多修改策略选择和自己的实现，不改标准钱包入口的主流程。

## 模块边界

`wallet-api/src/domain/chain/adapter/evm_fee/`：

- `mod.rs`：接收现有 `EthChain`、转账请求、Context、backend 和币价；执行余额检查、读取 tracker、构造 `TransferOpt`、估算执行 gas，并组合费用报价。模块内部只在一个位置按 `ChainCode` 选择附加费用策略。
- `scroll.rs`：只负责通过当前配置的 Scroll RPC 调用 `scroll_estimateL1DataFee`，传递与主币/ERC-20 转账对应的交易对象，并解析 wei 返回值。RPC 错误、格式错误和零费用均返回错误，不伪装成完整报价。
- `tests.rs`：放费用策略与报价形状测试；Scroll RPC 请求测试放在 `scroll.rs` 的测试模块或同目录测试文件，不让生产文件被 HTTP 测试夹具淹没。

`transaction_adapter.rs` 的 `Self::Ethereum` 报价分支改为一次 `evm_fee::estimate_transfer_fee(...)` 调用，不保留 Scroll 特例。`response_vo` 只保留把一个附加的主币费用加到三档 `estimateFee`/`maxFee` 的通用操作，`feeSetting` 仍仅代表 L2 执行 gas，避免把 L1 费用误当成可签名的 gas 参数。

## 五条链的当前处理

| 链 | 本轮策略 | 本轮结果 |
| --- | --- | --- |
| Polygon | 现有执行费路径 | 行为不变 |
| Avalanche | 现有执行费路径 | 行为不变 |
| Scroll | 执行费 + Scroll L1 数据费 | 本轮实现并测试 |
| Arbitrum | 暂沿用现有执行费路径 | 附加费用待后续批次核实，不声称报价完整 |
| Base | 暂沿用现有执行费路径 | 附加费用待后续批次核实，不声称报价完整 |

策略选择是一个集中 `match`，不是在余额检查、报价转换和 RPC 调用处各写一串 `if else`。Arbitrum/Base 的具体节点接口与计费方式在各自批次验证后再增加专属实现；本轮不写猜测公式。

## 数据与错误流

1. 标准钱包现有费用入口进入 EVM 费用模块；余额检查和后端 tracker 行为保持原样。
2. 构造与实际转账一致的 `TransferOpt`。普通转账使用 `to`/`value`；ERC-20 使用代币合约地址和 ABI calldata。
3. 对 Scroll，从当前链节点配置取得 RPC 地址与鉴权头，查询 L1 数据费；其他链按本轮策略不额外查询。
4. 使用现有 `eth_estimateGas` 得到执行 gas，按现有 tracker 生成三档执行费。Scroll 再把同一个 L1 数据费加进每档的预计与最高总金额；不修改每档 `feeSetting`。
5. 若 Scroll L1 RPC 不可用或结果无效，直接让报价失败，不降级为 0，也不向前端返回看似完整的低估报价。

现有 `TransactionAdapter` 仅保存 `EthChain`，没有保留所选 RPC 端点。因此本轮由费用模块通过现有节点工厂读取当前 Scroll 节点配置，不为此改动所有 adapter 枚举匹配分支。节点选择一致性在真实节点验证中检查；若后续确认需要固定同一端点，再单独调整 adapter 的端点持有方式。

## 测试与停止条件

- 离线测试覆盖主币和 ERC-20 请求对象、L1 RPC 正常/错误/无效/零值、三档总额和 `feeSetting` 不变、非 Scroll 报价行为不变。
- 运行 `cargo test -p wallet-api --lib --no-default-features`、`cargo check -p wallet-api --lib --no-default-features`、`cargo fmt --all -- --check`、`git diff --check`。
- 更新标准钱包费用断言矩阵。真实 Scroll 节点的只读请求单独验证，不混进默认测试。
- 完成后保留在 2.2 分支未提交状态，供人工审查；不合并、不推送。
