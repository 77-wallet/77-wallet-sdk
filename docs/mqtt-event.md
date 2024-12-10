# MQTT消息数据结构

公用结构体

```rust
struct Message<T>{
  // 客户端标识
  client_id:String,
  // 设备号
  sn:String,
  // 设备类型
  device_type:String,
  // 业务类型(一个枚举值)
  biz_type: String，
  // 业务数据 T 泛型
  body:T
}
```

## 多签相关

#### 订单多签-发起签名受理

> ORDER_MULTI_SIGN_ACCEPT(3,"订单多签-发起签名受理"),

发起发触发api，将多签的账号信息同步给参与方，参与方将多签账号写入本地数据库。

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
    /// 签名阀值
    pub(crate) threshold: i32,
    pub(crate) memeber: Vec<wallet_database::sqlite::logic::multisig_account::Member>,
}

#[derive(Debug, serde::Deserialize, Serialize)]
pub struct Member {
    // 参与方名称
    pub name: String,
    // 参与方地址
    pub address: String,
    // 确认状态(1已确认 0未确认)
    pub confirmed:i8,
    // 公钥
    pub pubkey:String,
}

```

- 示例

  ```json
  {
          "clientId": "wenjing",
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
              "memeber": [{
                  "name": "wenjing",
                  "address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
                  "confirmed": 0,
                  "pubkey":"xx",
              },
              {
                  "name": "bob",
                  "address": "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren",
                  "confirmed": 1,
                  "pubkey":"",
              }]
          }
      }
  ```

#### 订单多签-发起签名受理完成-消息通知

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
          "clientId": "wenjing",
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

#### 订单多签-服务费收取完成
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
          "clientId": "wenjing",
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



#### 订单多签-取消签名

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
          "clientId": "wenjing",
          "sn": "device458",
          "deviceType": "typeC",
          "bizType": "ORDER_MULTI_SIGN_CANCEL",
          "body": {
              "multisigAccountId": "order-1",
          }
      }
  ```





#### 订单多签-账户创建完成

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
          "clientId": "wenjing",
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

  



#### 多签转账-发起签名受理

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
          "clientId": "wenjing",
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

  

#### 多签转账-发起签名受理完成

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
          "clientId": "wenjing",
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

#### 账变通知 

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
    // 交易方式 0转入 1转出
    pub transfer_type: i8,
    // 交易类型 1:普通交易，2:部署多签账号 3:服务费
    pub tx_kind: i8,
    // 发起方
    pub from_addr: String,
    // 接收方
    pub to_addr: String,
    // 合约地址
    pub token: String,
    // 交易额
    pub value: String,
    // 手续费
    pub transaction_fee: String,
    // 交易时间
    pub transaction_time: String,
    // 交易状态 1-pending 2-成功 3-失败
    pub status: i8,
    // 是否多签 1-是，0-否
    pub is_multisig: i32,
    // 队列id
    pub queue_id: String,
    // 块高
    pub block_height: String,
    // 备注
    pub notes: String,
}

```

- 示例

  ```json
  {
          "clientId": "wenjing",
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
              "value": "1000000000000000000",
              "transactionFee": "21000",
              "transactionTime": "2024-07-30T12:34:56Z",
              "status": 2,
              "isMultisig": 1,
              "queueId": "queue123",
              "blockHeight": "12345678",
              "notes": "Payment for services"
          }
      }
  ```

  

#### 账户余额初始化

> INIT(2,"账户余额初始化")

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
      "clientId": "wenjing",
      "sn": "wenjing",
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
  
  

#### 代币价格变动

> TOKEN_PRICE_CHANGE(8,"代币价格变动"),

```rust
// biz_type = TOKEN_PRICE_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceChange {
  	// 代币id
    pub id: String,
    // 链码
    pub chain_code: String,
    // 代币编码
    pub code: String,
    // 默认代币
    pub default_token: bool,
    // 启用状态
    pub enable: bool,
    // 市值
    pub market_value: f64,
    // 主币
    pub master: bool,
    // 代币名称
    pub name: String,
    // 单价
    pub price: f64,
    // 可以状态
    pub status: bool,
    // 代币合约地址
    pub token_address: String,
    // 精度
    pub unit: u8,
}

```

- 示例

  ```json
  {
      "clientId": "wenjing",
      "sn": "wenjing",
      "deviceType": "ANDROID",
      "bizType": "TOKEN_PRICE_CHANGE",
      "body": {
          "id": "123123123",
          "chainCode": "polygon",
          "code": "chain",
          "defaultToken": false,
          "enable": true,
          "marketValue": 6644971.07,
          "master": false,
          "name": "Chain Games",
          "price": 0.021205427084188898,
          "status": false,
          "tokenAddress": "0xd55fce7cdab84d84f2ef3f99816d765a2a94a509",
          "unit": 18,
      }
  }
  ```

  











