# 77Wallet 2.2 新增 EVM 链改动说明

## 1. 目的与范围

本文根据 `77_wallet_core` 和 `77_wallet_sdk` 当前代码评估编写，作为其他工作对话实施
2.2 新增 EVM 链的交接文档。本阶段先修改 `wallet_core`，后台配置未准备不阻塞开发；
Core 完成后再由独立任务接入 SDK。

本次不包括 APPID 网络激活、通知、UI、能量趋势图、`SigningBackend`、硬件钱包协议、
已有链签名架构迁移或 legacy 交易能力扩展。

原始需求中的 Avalanche C-Chain 重复出现，按一条链处理，实际新增六条链。

## 2. ChainCode 约定

使用小写官方网络名称作为内部 `chainCode`。第一版不接受旧品牌名、币种符号或简称。

| 网络 | canonical `chainCode` | 不作为 canonical code |
| --- | --- | --- |
| Polygon PoS | `polygon` | `matic` |
| Avalanche C-Chain | `avalanche` | `avax` |
| Fantom Opera | `fantom` | `ftm` |
| Arbitrum One | `arbitrum` | `arb` |
| Base | `base` | 无 |
| Scroll | `scroll` | 无 |

解析保持与当前 `eth`、`bnb` 一致：大小写敏感，只接受 canonical code，其他值返回
`UnknownChainCode`。

## 3. 代码现状与结论

### 3.1 链类型和派生

当前 `wallet-types/src/chain/enums/chain.rs` 只包含 Ethereum 和 BNB 两条 EVM 链，
`ChainCodes::try_from(ETH_TYPE)` 也只返回这两条链。

当前 `EthereumInstance` 已经：

- 使用 `m/44'/60'/...` 派生路径；
- 保存实际 `ChainCode`；
- 被 Ethereum 和 BNB 共同复用；
- 使用通用 EVM 地址生成算法。

所以六条新链不应分别新增 Instance，而应全部复用 `EthereumInstance`。为避免扩大公共
API，本次保留 `ChainObject::Eth` 和 `ChainObject::Bnb`，不进行 `ChainObject::Evm`
等破坏性重构。

现有 `wallet-chain-instance/src/instance/mod.rs` 的 `gen_gen_address()` 对
`ChainObject::Eth` 硬编码使用 `ChainCode::Ethereum`。新链接入时必须改成读取实例的
实际 `chain_code`，否则地址值虽然正确，地址生成器报告的链标识会错误。

### 3.2 EVM 交易实现

当前 `wallet-chain-interact` 的 `EthChain` 接收任意 `ChainCode`。原生币和 ERC20 共同
使用 Alloy `TransactionRequest`，并通过标准 `eth_*` RPC 完成交易：

```text
构造 TransactionRequest
→ 设置费用和 nonce
→ RPC 查询 eth_chainId
→ 本地私钥签名 envelope
→ EIP-2718 编码
→ eth_sendRawTransaction
```

因此新链不需要独立 provider、operations、签名器或广播实现。

当前唯一明确的链级费用特例是 BNB。`set_transaction_fee()` 只对
`ChainCode::BnbSmartChain` 特殊处理，新增链应走普通 EIP-1559 分支，不能复制 BNB
特例。

### 3.3 chain ID 和交易类型

本次不在 `ChainCode` 或常量中维护 chain ID，但签名过程仍然使用 chain ID。现有 Core
会在签名前调用 `eth_chainId`，将 RPC 返回值写入交易后再签名。

准确约束是：

> 新增链类型阶段不硬编码 chain ID；签名阶段继续从目标 RPC 动态获取 chain ID。

固定签名测试应把 chain ID 作为离线测试输入，不能调用真实 RPC。

当前稳定路径设置 `max_fee_per_gas` 和 `max_priority_fee_per_gas`，属于现有
EIP-1559/type-2 路径。代码中没有本需求对应的独立 legacy/type-0 构造入口，因此本次
保持现有行为，不增加 legacy 支持。若产品后续明确要求 legacy，需单独立项。

## 4. Core 实施批次

按照 Core 仓库的变更门禁，跨 crate 工作必须分批实施。

### Batch 1：`wallet-types` 链类型

预计文件：

- `wallet-types/src/constant.rs`
- `wallet-types/src/chain/enums/chain.rs`
- 同模块或最近测试模块中的聚焦测试

改动：

1. 增加六个 canonical chain-code 常量。
2. 增加 `Polygon`、`Avalanche`、`Fantom`、`Arbitrum`、`Base`、`Scroll`。
3. 对称扩展 `TryFrom<&str>` 和 `Display`。
4. 将六个枚举加入 `ChainCodes::try_from(ETH_TYPE)`。
5. 不增加 `matic`、`avax`、`ftm`、`arb` 等别名。

测试矩阵：

- 六个 canonical code 的解析、展示和 round trip；
- `eth`、`bnb` 及非 EVM 旧链行为不变；
- 非 canonical 别名和未知字符串继续报错；
- `ETH_TYPE` 包含 ETH、BNB 和六条新链，且没有重复。

验证：

```text
cargo test -p wallet-types chain
cargo check -p wallet-types
cargo fmt --all -- --check
git diff --check
```

停止条件：链类型契约完成并通过测试；不修改其他 crate。

### Batch 2：`wallet-chain-instance` 地址派生

预计文件：

- `wallet-chain-instance/src/instance/mod.rs`
- `wallet-chain-instance/src/derivation_path.rs`，仅在现有链族测试需要时修改
- 聚焦的 EVM 实例和地址测试

改动：

1. 将六个新 `ChainCode` 路由到 `EthereumInstance`。
2. 继续使用 Ethereum/BIP-44 coin type 60 派生路径。
3. 在 `EthereumInstance.chain_code` 中保存实际新链。
4. 让 `ChainObject::gen_gen_address()` 使用实例的实际 `chain_code`。
5. 更新新增枚举造成的 exhaustive match，不改变非 EVM 行为。

对每条新链验证：

- `ChainObject::try_from` 使用 `AddressType::Other` 可成功构造；
- `ChainObject::chain_code()` 返回请求的新链，不返回 `eth`；
- 使用现有 Ethereum 派生路径；
- 相同固定测试 seed/path 与 Ethereum 生成相同地址值；
- KeyPair 和地址生成器均保留请求的新链标识。

同时保留 Ethereum 和 BNB 回归。测试数据必须明确为固定非生产数据，禁止使用真实助记
词或私钥。

验证：

```text
cargo test -p wallet-chain-instance evm
cargo check -p wallet-chain-instance
cargo fmt --all -- --check
git diff --check
```

停止条件：六条链均能生成正确且链标识准确的 EVM 地址；不修改 RPC、费用、签名架构
或 SDK adapter。

### Batch 3：Core 编译闭环和签名基线

本批次不新增交易架构，只确认扩展枚举后现有 EVM 实现仍然成立。

范围：

- 修复新增枚举造成的必要 exhaustive match；
- 增加现有 EVM 构造和本地签名的聚焦离线测试；
- 除非测试证明存在缺口，否则不修改生产交易实现。

测试要求：

- 固定 chain ID、nonce、费用、地址和数额；
- 覆盖原生币和 ERC20 `TransactionRequest`；
- 验证现有 EIP-1559 raw transaction 和交易 hash；
- 至少用一条新链枚举验证 `EthChain` 保留实际链标识；
- 不使用真实 RPC、账户、密钥或余额；
- 不增加 legacy、`SigningBackend` 或硬件签名测试。

固定向量用于建立加链回归基线、降低未来硬件签名迁移成本，不代表本次采用新架构。

验证：

```text
cargo test -p wallet-chain-interact --lib
cargo check -p wallet-chain-interact
cargo fmt --all -- --check
git diff --check
```

停止条件：受影响 Core crate 编译通过，现有 EIP-1559 本地签名基线通过；不新增 RPC
endpoint、后台配置或硬件接口。

## 5. Core 验收标准

- 六个 chainCode 的解析和展示符合本文约定；
- 六条链均归入 ETH/BIP-44 coin type 60；
- 六条链复用 `EthereumInstance`，没有重复实现；
- 地址正确，KeyPair 和地址生成器保留实际链标识；
- Ethereum、BNB 和非 EVM 旧链行为不变；
- BNB 费用特例没有扩散到新链；
- chain ID 继续由 RPC 动态获取，没有新增硬编码表；
- 现有 EIP-1559 本地签名和编码基线通过；
- 未引入 legacy、`SigningBackend`、硬件协议或 SDK 业务改动；
- 测试全部离线，不依赖真实 RPC 或敏感信息；
- 每个批次分别通过最小测试、check、fmt 和 diff check。

## 6. SDK 后续影响

Core 合入并提供 Git revision 后，再由独立 SDK 任务接入。

SDK 的 `EthTx` 已支持传入任意 `ChainCode`，但大量业务判断仍写死为：

```rust
matches!(chain, ChainCode::Ethereum | ChainCode::BnbSmartChain)
```

或：

```rust
matches!(chain_code, "eth" | "bnb")
```

影响 adapter、nonce、广播恢复、归集、提币、手续费补充、费用准入及部分状态机。SDK
接入不能只修改 adapter factory，应先建立唯一的 EVM 分类入口，再分批迁移：

1. adapter 选择与链初始化；
2. nonce 与广播恢复；
3. 归集；
4. 提币；
5. 手续费补充；
6. 固定交易回归和编译闭环。

预计涉及多个 flow 和超过 10 个文件，必须按 SDK 根目录 `AGENTS.md` 拆批实施。

## 7. 与硬件钱包的边界

2.2 新链沿用当前本地签名架构，不提前引入 `SigningBackend`。本次只要求：

- 不新增分散的私钥获取和签名入口；
- 为待签交易、签名结果、raw transaction 和 hash 保留固定测试基线；
- 不大规模重构构造、签名和广播职责。

硬件钱包应在 2.2 合入后，从最新分支开始，先用 TRON 验证外部签名抽象，再迁移
EVM。两项工作不得并行重构相同 Core 接口。

## 8. 交接要求

Core 工作对话开始前应读取：

- `77_wallet_core/AGENTS.md`；
- `77_wallet_core/docs/codex/testing.md`；
- `77_wallet_core/docs/codex/checklists/pr-definition-of-done.md`；
- 最近的子模块 `AGENTS.md`（如后续新增）。

Core 完成后向 SDK 工作对话提供：

- Git revision；
- 最终 ChainCode 对照表；
- 各批次测试及结果；
- 是否修改 `wallet-chain-interact` 生产代码；
- 尚未覆盖的交易类型或网络限制。

建议提交信息：

```text
feat(types): add 2.2 EVM chain codes
feat(chain-instance): support new EVM chain derivation
test(chain-interact): add EVM signing vectors
```
