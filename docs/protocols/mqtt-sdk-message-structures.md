# SDK 内部 MQTT 消息结构

本文记录 SDK 内部通过 MQTT 同步的业务消息结构，当前主要覆盖多签相关
消息。服务端推送给商户侧 SDK 的订单、结果和命令类协议见
[mqtt-merchant-push-protocol.md](mqtt-merchant-push-protocol.md)。

## 消息格式

SDK 内部消息使用统一外层结构：公共字段描述消息来源和业务类型，
`body` 承载具体业务数据。

```rust
struct Message<T> {
    // 消息 ID，用于任务去重和 ACK
    msg_id: String,
    // 业务类型，一个枚举值
    biz_type: String,
    // 业务数据
    body: T,
    // 客户端标识
    client_id: String,
    // 设备号
    sn: String,
    // 设备类型
    device_type: String,
    // 钱包类型，API 钱包账变会用它区分处理路径
    wallet_type: Option<WalletType>,
}
```

字段对应 JSON 使用 camelCase：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `msgId` | String | 消息 ID，用于任务去重和 ACK |
| `bizType` | String | 业务类型 |
| `body` | Object | 业务数据，结构由 `bizType` 决定 |
| `clientId` | String | 客户端标识 |
| `sn` | String | 设备号 |
| `deviceType` | String | 设备类型 |
| `walletType` | String | 钱包类型，可空 |

## 业务类型速查

下表按 `wallet-api/src/messaging/mqtt/message.rs` 的 `BizType` 整理。

| `bizType` | 说明 |
| --- | --- |
| `ORDER_MULTI_SIGN_ACCEPT` | 订单多签发起签名受理 |
| `ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG` | 订单多签发起签名受理完成通知 |
| `ORDER_MULTI_SIGN_SERVICE_COMPLETE` | 订单多签服务费收取完成 |
| `ORDER_MULTI_SIGN_CANCEL` | 订单多签取消签名 |
| `ORDER_MULTI_SIGN_CREATED` | 订单多签账户创建完成 |
| `ORDER_MULTI_SIGN_ALL_MEMBER_ACCEPTED` | 订单多签所有成员已确认 |
| `MULTI_SIGN_TRANS_ACCEPT` | 多签转账发起签名受理 |
| `MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG` | 多签转账签名结果同步 |
| `MULTI_SIGN_TRANS_CANCEL` | 多签转账取消 |
| `MULTI_SIGN_TRANS_EXECUTE` | 多签交易已进入执行/确认流程 |
| `ACCT_CHANGE` | 普通钱包或 API 钱包账变 |
| `PERMISSION_ACCEPT` | 权限变更同步 |
| `CLEAN_PERMISSION` | 清理多签账号原权限 |
| `BULLETIN_MSG` | 公告消息 |
| `RPC_ADDRESS_CHANGE` | RPC 节点变更 |
| `TOKEN_PRICE_CHANGE` | 代币价格变动 |
| `AWM_ORDER_TRANS` | API 钱包订单消息，详见商户侧推送协议 |
| `AWM_ORDER_TRANS_RES` | API 钱包订单结果消息，详见商户侧推送协议 |
| `AWM_CMD_RSC_RES` | API 钱包资源结果消息，详见商户侧推送协议 |
| `AWM_CMD_ADDR_EXPAND` | API 钱包地址扩容消息，详见商户侧推送协议 |
| `AWM_CMD_FEE_RES` | API 钱包手续费结果消息，详见商户侧推送协议 |
| `AWM_CMD_ACTIVE` | API 钱包激活消息，详见商户侧推送协议 |
| `AWM_CMD_DEV_CHANGE` | API 钱包设备变更消息，详见商户侧推送协议 |

说明：代码中还保留 `AWM_CMD_UID_UNBIND`、`ADDRESS_USE` 等分支，
当前业务未使用，本文不作为正式消息结构展开。

## Topic 入口

代码中 `Topic::from_bytes_v3` 支持以下 MQTT topic：

| Topic | 说明 |
| --- | --- |
| `wallet/common/{clientId}` | 通用 SDK 消息 |
| `wallet/order/{clientId}` | 订单、多签、账变、权限类消息 |
| `wallet/bulletin/{clientId}` | 公告消息 |
| `wallet/switch` | 钱包切换广播 |
| `wallet/token` | 代币价格变动 |
| `wallet/rpc/change` | RPC 节点变更 |
| `wallet/chain/change` | 链配置变更 |
| `aw/merchant/trans/{clientId}` | 商户侧交易类推送 |
| `aw/merchant/cmd/{clientId}` | 商户侧命令类推送 |

## 多签消息

### 订单多签-发起签名受理

> ORDER_MULTI_SIGN_ACCEPT(3,"订单多签-发起签名受理"),

发起方触发 API 后，将多签账号信息同步给参与方；参与方收到后将多签账号
写入本地数据库。

```rust
// biz_type = ORDER_MULTI_SIGN_ACCEPT
#[derive(Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignAccept {
    /// uuid
    pub(crate) id: String,
    /// 钱包名称
    pub(crate) name: String,
    /// 发起方地址
    pub(crate) initiator_addr: String,
    /// 多签钱包地址
    pub(crate) address: String,
    /// 链编码
    pub(crate) chain_code: String,
    /// 签名阈值
    pub(crate) threshold: i32,
    /// 地址类型
    pub(crate) address_type: String,
    /// 参与成员列表。注意：当前协议 JSON 字段名为历史拼写 `memeber`。
    #[serde(rename = "memeber")]
    pub(crate) member: Vec<wallet_database::entities::multisig_member::MemberVo>,
}

#[derive(Debug, serde::Deserialize, Serialize)]
pub struct Member {
    // 参与方名称
    pub name: String,
    // 参与方地址
    pub address: String,
    // 确认状态，1 已确认，0 未确认
    pub confirmed: i8,
    // 公钥
    pub pubkey: String,
}
```

- 示例

  ```json
  {
          "clientId": "666",
          "sn": "device456",
          "deviceType": "ANDROID",
          "bizType": "ORDER_MULTI_SIGN_ACCEPT",
          "body": {
              "id": "uuid-1",
              "name": "Wallet1",
              "initiatorAddr": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
              "address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
              "chainCode": "tron",
              "threshold": 2,
              "addressType": "p2wsh",
              "memeber": [{
                  "name": "666",
                  "address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
                  "confirmed": 0,
                  "pubkey": "xx"
              },
              {
                  "name": "bob",
                  "address": "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren",
                  "confirmed": 1,
                  "pubkey": ""
              }]
          }
      }
  ```

### 订单多签-发起签名受理完成-消息通知

> ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG(5,"订单多签-发起签名受理完成-消息通知")

同步参与状态api：发起方接受到参与方的确认后，发起通知参与方，参与方修改状态。

```rust
// biz_type = ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderMultiSignAcceptCompleteMsg {
    /// 多签账户id
    multisig_account_id: String,
    /// 参与状态(同意true,不同意false)
    status: bool,
    /// 参与方地址
    address: Vec<String>,
    // 参与人全部确认完
    accept_status: bool, 
    accept_address_list: Vec<Confirm>, 
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Confirm {
    /// 参与方地址
    address: String,
    pubkey: String,
    /// 参与方确认状态
    status: i32,
}
```

- 示例

  ```json
  {
          "clientId": "666",
          "sn": "device457",
          "deviceType": "ANDROID",
          "bizType": "ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG",
          "body": {
              "status": 1,
              "multisigAccountId": "order-1",
              "address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
              "acceptStatus": false,
              "acceptAddressList": []
          }
      }
  ```

### 订单多签-服务费收取完成

> ORDER_MULTI_SIGN_SERVICE_COMPLETE(6,"订单多签-服务费收取完成")
当手续费或服务费完成后，通知参与方修改状态。多签账号已启用。

```rust
stuct Body{
  // 多签账户id
  multisig_account_id:String，
  // 多签账号结果 true 多签账号或服务费执行完成  false 失败
  status:bool，
  // 1手续费 2服务费
  r#type: u8,
}
```

- 示例

  ```json
      {
          "clientId": "666",
          "sn": "device458",
          "deviceType": "typeC",
          "bizType": "ORDER_MULTI_SIGN_SERVICE_COMPLETE",
          "body": {
              "multisigAccountId": "order-1",
              "status": true,
              "type": "1"
          }
      }
  ```

------

### 订单多签-取消签名

> ORDER_MULTI_SIGN_CANCEL(4,"订单多签-取消签名")

```rust
struct Body{
  // 多签账户id
  multisig_account_id:String，
}
```

- 示例

  ```json
      {
          "clientId": "666",
          "sn": "device458",
          "deviceType": "typeC",
          "bizType": "ORDER_MULTI_SIGN_CANCEL",
          "body": {
              "multisigAccountId": "order-1",
          }
      }
  ```

### 订单多签-账户创建完成

> ORDER_MULTI_SIGN_CREATED(7,"订单多签-账户创建完成")

当手续费或服务费完成后，通知参与方修改状态。多签账号已启用。

```rust
// biz_type = ORDER_MULTI_SIGN_CREATED
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderMultiSignCreated {
    /// 多签账户id
    multisig_account_id: String,
    /// 多签账户地址
    multisig_account_address: String,
    /// 地址类型
    address_type: String,
    /// btc solana 盐
    salt: String,
    /// solana 管理地址
    authority_addr: String,
}
```

- 示例

  ```json
      {
          "clientId": "666",
          "sn": "device458",
          "deviceType": "typeC",
          "bizType": "ORDER_MULTI_SIGN_CREATED",
          "body": {
              "multisigAccountId": "order-1",
              "multisigAccountAddress": "asdasdasdasd",
              "addressType": "p2wsh",
             "salt": "asdasd",
             "authorityAddr": "sadasdasd"
          }
      }
  ```

### 多签转账-发起签名受理

> MULTI_SIGN_TRANS_ACCEPT(9,"多签转账-发起签名受理"),

发起方发起一笔交易后，将交易信息同步给参与方

```rust
// biz_type = MULTI_SIGN_TRANS_ACCEPT
#[derive(Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransAccept {
    pub id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    pub symbol: String,
    pub chain_code: String,
    pub token_addr: Option<String>,
    pub msg_hash: String,
    pub tx_hash: String,
    pub raw_data: String,
    /// 0待签名 1待执行 2已执行
    pub status: i8,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

```

- 示例

  ```json
  {
          "clientId": "666",
          "sn": "device460",
          "deviceType": "typeE",
          "bizType": "MULTI_SIGN_TRANS_ACCEPT",
          "body": {
              "id": "tx123456789",
              "fromAddr": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
              "toAddr": "0xReceiverAddress",
              "value": "1000",
              "expiration": 1698806400,
              "symbol": "eth",
              "chainCode": "eth",
              "tokenAddr": null,
              "msgHash": "0xMessageHash",
              "txHash": "0xTransactionHash",
              "rawData": "raw transaction data",
              "status": 0,
              "notes": "This is a test transaction",
              "createdAt": "2024-07-30T12:34:56Z"
          }
      }
  ```

### 多签转账-发起签名受理完成

> MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG(10,"多签转账-发起签名受理完成")

参与方签名后，将信息同步给所有多签参与者。

```rust
// biz_type = MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransAcceptCompleteMsg {
    #[serde(flatten)]
    body: Vec<MultiSignTransAcceptCompleteMsgBody>,
}

#[derive(Debug, serde::Deserialize, Serialize)]
pub struct MultiSignTransAcceptCompleteMsgBody {
    pub queue_id: String,
    pub address: String,
    pub signature: String,
    /// 0未签 1签名  2拒绝
    pub status: i8,
}
```

- 示例

  ```json
  {
          "clientId": "666",
          "sn": "device460",
          "deviceType": "typeE",
          "bizType": "MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG",
          "body": [{
              "queueId": "tx123456789",
              "address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
              "signature": "signature-1",
              "status": 1
          }]
      }
  ```

## 其他

### 账变通知

> ACCT_CHANGE(1,"帐变")

```rust
// biz_type = ACCT_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcctChange {
    // 交易hash
    pub tx_hash: String,
    // 链码
    pub chain_code: String,
    // 币种符号
    pub symbol: String,
    // 交易方式 0 转入，1 转出，2 初始化
    pub transfer_type: i8,
    // 交易类型 1:普通交易，2:部署多签账号 3:服务费
    pub tx_kind: i8,
    // 发起方
    pub from_addr: String,
    // 接收方
    #[serde(default)]
    pub to_addr: String,
    // 合约地址
    #[serde(default)]
    pub token: Option<String>,
    // 交易额
    #[serde(default)]
    pub value: f64,
    // 手续费
    pub transaction_fee: f64,
    // 交易时间
    #[serde(default)]
    pub transaction_time: String,
    // 交易状态 true 成功，false 失败
    pub status: bool,
    // 是否多签 1-是，0-否
    #[serde(default)]
    pub is_multisig: i32,
    // 队列id
    #[serde(default)]
    pub queue_id: String,
    // 块高
    pub block_height: i64,
    // 备注
    #[serde(default)]
    pub notes: String,
    // 带宽消耗
    #[serde(default)]
    pub net_used: u64,
    // 能量消耗
    #[serde(default)]
    pub energy_used: Option<u64>,
    // 额外信息
    pub extra: Option<serde_json::Value>,
}

```

- 示例

  ```json
  {
          "clientId": "666",
          "sn": "device460",
          "deviceType": "typeE",
          "bizType": "ACCT_CHANGE",
          "body": {
              "txHash": "0x1234567890abcdef",
              "chainCode": "ETH",
              "symbol": "ETH",
              "transferType": 0,
              "txKind": 1,
              "fromAddr": "0xabcdef1234567890",
              "toAddr": "0x1234567890abcdef",
              "token": "0xabcdef1234567890abcdef1234567890abcdef",
              "value": 1.0,
              "transactionFee": 0.001,
              "transactionTime": "2024-07-30 12:34:56",
              "status": true,
              "isMultisig": 1,
              "queueId": "queue123",
              "blockHeight": 12345678,
              "notes": "Payment for services",
              "netUsed": 0,
              "energyUsed": 0,
              "extra": null
          }
      }
  ```

### 账户余额初始化（未接入）

> INIT(2,"账户余额初始化")

`INIT` 结构在旧文档中保留，但当前 `BizType` 和 `Body` 中已注释，
本地 MQTT 入口不会分发该消息。

```rust
// biz_type = INIT
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    // 地址
    pub address: String,
    // 链码
    pub chain_code: String,
    // 余额
    pub balance: String,
    // 代币编码
    pub code: String,
    // 合约地址
    pub token_address: Option<String>,
    // 代币精度
    pub decimals: u8,
}
```

- 示例

  ```json
  {
      "clientId": "666",
      "sn": "666",
      "deviceType": "ANDROID",
      "bizType": "INIT",
      "body": [
          {
              "address": "TGyw6wH5UT5GVY5v6MTWedabScAwF4gffQ",
              "balance": 4000002,
              "chainCode": "tron",
              "code": "sadsadsad",
             "tokenAddress": "",
             "decimals": 6
          }
      ]
  }
  ```
  
### 代币价格变动

> TOKEN_PRICE_CHANGE(8,"代币价格变动"),

```rust
// biz_type = TOKEN_PRICE_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceChange {
    pub body: TokenPriceChangeBody,
}

```

- 示例

  ```json
  {
      "body": {
          "chainCode": "polygon",
          "symbol": "chain",
          "price": 0.021205427084188898,
          "tokenAddress": "0xd55fce7cdab84d84f2ef3f99816d765a2a94a509",
          "unit": 18,
          "swappable": false
      }
  }
  ```

### 链变动

> CHAIN_CHANGE(,"链变动"),

```rust
// biz_type = CHAIN_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChainChange(Vec<ChainUrlInfo>);

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChainUrlInfo {
    /// 查看链上地址URL
    pub address_url: Option<String>,
    /// 查看链上hash URL
    pub hash_url: Option<String>,
    #[serde(rename = "code")]
    pub chain_code: String,
    pub enable: bool,
    pub name: String,
    pub master_token_code: Option<String>,
}

```

- 示例

  ```json
  {
      "clientId": "666",
      "sn": "666",
      "deviceType": "ANDROID",
      "bizType": "CHAIN_CHANGE",
      "body": [{
        "chainCode": "btc",
        "rpcAddressInfoBodyList": [{
          "id": "676b6e486e07fa2e51a746ca",
          "name": "app_btc",
          "url": "https://apprpc.safew.cc/btc"
        }]
      }, {
        "chainCode": "sol",
        "rpcAddressInfoBodyList": [{
          "id": "676b6e816e07fa2e51a746cc",
          "name": "APP_SOL",
          "url": "https://apprpc.safew.cc/sol"
        }]
      }, {
        "chainCode": "bnb",
        "rpcAddressInfoBodyList": [{
          "id": "676b6e366e07fa2e51a746c9",
          "name": "app_bnb",
          "url": "https://apprpc.safew.cc/bnb"
        }]
      }, {
        "chainCode": "eth",
        "rpcAddressInfoBodyList": [{
          "id": "676b6e906e07fa2e51a746cd",
          "name": "app_eth",
          "url": "https://apprpc.safew.cc/eth"
        }]
      }, {
        "chainCode": "ltc",
        "rpcAddressInfoBodyList": [{
          "id": "677e7e80230d86ab7c0851f8",
          "name": "app_ltc",
          "url": "https://apprpc.safew.cc/ltc"
        }]
      }, {
        "chainCode": "tron",
        "rpcAddressInfoBodyList": [{
          "id": "676b6e566e07fa2e51a746cb",
          "name": "app_tron",
          "url": "https://apprpc.safew.cc/tron"
        }]
      }]
  }
  ```

## 地址余额不足通知（AWM_ADDRESS_INSUFFICIENT）

后台通知 body 字段如下；SDK 保存通知，详情接口透传 `waddressType`。

| 字段 | 后台类型 | 说明 |
| --- | --- | --- |
| remaining | BigDecimal | 剩余余额，单位为币 |
| token | String | 代币合约；原生币为空字符串 |
| symbol | String | 代币编码 |
| merchantName | String | 商户名称 |
| waddressType | String | `"1"`：正常地址；`"2"`：风险地址 |
| thresholdValue | BigDecimal | 触发阈值，单位为币 |
| targetAddress | String | 地址 |
| chainCode | String | 地址所在的链编码 |

后台 Java 字段名为 `wAddressType`，SDK 使用的 JSON 字段名为 `waddressType`。
此字段区分正常地址与风险地址，不是 API 钱包收款/出款类型，也不是链上地址格式。

取值定义来源：用户于 2026-10-26 提供的后台 Java 字段注释
`// 1 正常地址，2 风险地址`；适用于本通知及其 SDK 详情透传字段。
