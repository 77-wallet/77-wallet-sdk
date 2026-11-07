# API Wallet Console 手工测试指南

`api_wallet_console` 是 `wallet-api` 里的本地 egui 桌面测试工具，用来替代反复修改
`wallet-api/examples` 和 smoke test 参数的手工测试方式。

它适合真实 backend / MQTT / 本地 `test_data` 参与的 smoke-live 验证，不属于默认 CI
自动化测试。

## 启动

在 workspace 根目录执行：

```sh
cargo run -p wallet-api --bin api_wallet_console
```

首次启动 worker 时可能需要编译。UI 会自动刷新，不需要移动鼠标触发重绘。

## 数据目录

每个 client 使用独立的本地数据目录：

```text
wallet-api/test_data/test_data_config/
wallet-api/test_data/test_data_client1/
wallet-api/test_data/test_data_client4/
```

worker 会按当前 client 初始化 xlog：

```rust
let _ = wallet_api::xlog::init_log(Some("info"), &"app_code", &dirs, "sn").await;
```

日志目录位于对应 client 的：

```text
wallet-api/test_data/test_data_<client>/log/
```

## 界面结构

### Clients

左侧是 client 列表。

- `default` 对应 `config.toml`
- `client1` 对应 `client1.toml`
- `client4` 对应 `client4.toml`

每个 client 是独立 worker 进程，可以并行启动。选择 client 后点
`Start this client`。

### Observe Tabs

中间区域是观察页签：

- `Runtime`：设备 SN、设备初始化 App ID、绑定 App ID、Org ID、UID。
- `Wallets`：当前 client 本地 DB 中的 API 钱包，包含子账户钱包和出款钱包。
- `Balances`：按钱包查看本地资产余额。
- `Accounts`：按钱包查看账户地址。

这些观察页默认不做高风险操作，只用于确认当前状态。

### Actions

右侧是操作区，按组折叠：

- `Wallet`：导入配置钱包、从 DB 刷新钱包列表。
- `Bind`：import bind / scan bind。
- `Withdraw Review`：查询待审核提币订单，批量通过或拒绝。
- `Batch Transfer`：批量转账。

### Logs

底部是多 client terminal。

- 每个 client 一个日志窗格。
- 窗格之间可以左右拖动。
- Logs 整体高度可以上下拖动。
- `mqtt N` 表示该 client 收到的 MQTT / notify 数量。
- `Notify` 过滤只看 MQTT / notify 相关日志。

UI 会剥离 ANSI 颜色控制码，避免日志里出现 `[2m` 这类字符。

## 常见流程

### 1. 并行启动多个 client

1. 选择 `client1`。
2. 点 `Start this client`。
3. 选择 `client4`。
4. 点 `Start this client`。
5. 在底部 `client1` 和 `client4` terminal 中分别查看日志。

每个 client 是独立 worker 进程，用来避免 SDK 全局 context 冲突。

### 2. 查看钱包、账户地址和余额

1. 启动目标 client。
2. 打开 `Wallets` 页签。
3. 如果列表为空，右侧 `Actions -> Wallet` 中点 `Refresh wallets from DB`。
4. 点钱包行的 `Show accounts` 查看账户地址。
5. 点钱包行的 `Show balances` 查看本地余额。

账户地址按钱包分组展示：

- `subwallet`：子账户钱包。
- `withdraw`：出款钱包。

### 3. 复制子账户地址为英文逗号分隔

1. 打开 `Accounts` 页签。
2. 先对 `subwallet` 点 `Show` 或在 `Wallets` 页签点 `Show accounts`。
3. 在 `Subwallet comma copy` 中填写：
   - `from account`：从哪个 `account_id` 开始。
   - `count`：复制多少个。
4. 点 `Copy comma addresses`。

复制结果格式：

```text
addr1,addr2,addr3
```

### 4. 查看 MQTT 消息

1. 启动 client。
2. 看底部对应 client terminal 标题中的 `mqtt N`。
3. 如果 `N > 0`，说明收到 MQTT / notify。
4. 点 Logs 顶部的 `Notify` 过滤只看相关日志。

如果 `mqtt 0`，说明当前 worker 没收到 frontend notify。此时应检查：

- client 是否真正 ready。
- MQTT 是否连接成功。
- 是否出现 `MQTT_CONNECTED`、`KEEP_ALIVE`、`FETCH_BULLETIN_MSG` 等 notify。
- 是否有多 client 使用相同 MQTT session 导致 `SessionTakenOver`。

### 5. 查询和审核提币订单

1. 启动目标 client。
2. 展开 `Actions -> Withdraw Review`。
3. 点 `Refresh pending orders`。
4. 勾选订单。
5. 点：
   - `Approve selected`：通过审核，调用 `sign_api_withdrawal_order(trade_no)`。
   - `Reject selected`：拒绝审核，调用 `reject_api_withdrawal_order(trade_no)`。

支持：

- `Select all`：全选当前加载的待审核订单。
- `Clear selection`：清空选择。
- 手填 trade number fallback：没有勾选订单时，使用手填 trade_no 批量操作。

待审核列表默认查询：

```rust
page_api_withdraw_order(withdrawal_uid, vec![ApiWithdrawStatus::Init as u8], 0, 50)
```

### 6. 批量转账

1. 启动目标 client。
2. 打开 `Wallets` 页签，确认 `subwallet` 地址。
3. 展开 `Actions -> Batch Transfer`。
4. 设置：
   - `Chain`
   - `Amount`
   - `From`
   - `Symbol`
   - `Subwallet`
   - `Decimals`
   - `Concurrency`
   - `Interval ms`
   - `Fee setting`
5. 填写目标地址，一行一个。
6. 点 `Run transfer`。
7. 在确认弹窗中确认后执行。

如果已在 `Accounts` 页签加载账户地址，可以使用：

- `Copy addresses`
- `Use as targets`

减少手动复制错误。

## 注意事项

- 这是手工 live/smoke 工具，不要把它当默认自动化测试。
- 高风险动作都在 `Actions` 中，首页观察页默认不触发通过、拒绝、转账等动作。
- `Device Init App ID` 和 `Binding App ID` 不是同一个概念。
- `client4` 的绑定 App ID 使用 smoke 里的商户绑定参数，例如
  `8276baee61e14956bf8ad036e4a5efb3`。
- 如果按钮一直不可点，先看顶部是否显示 `Running`，以及底部日志是否有完成事件。
- 如果中文显示为方框，需要确认 UI 注册的 macOS 中文 fallback 字体存在。
