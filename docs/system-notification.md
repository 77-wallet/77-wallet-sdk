# SYSTEM NOTIFICATION

## 普通账户转账

type：1

content：

```rust
struct CommonAccountTransfer {
        wallet_name: String,
        uid: String,
        // 交易方式 0转入 1转出
        transfer_type: i8,
        // 交易状态 1-pending 2-成功 3-失败
        status: i8,
    }
```

```json
{
    "wallet_name": "example_wallet",
    "uid": "f394a0063e958aa7f0405360c584f24e",
    "transfer_type": 1,
    "status": 2
}
```



## 多签账户等待加入

type：2

content：

```rust
struct MultisigAccountBuildUp {
        uid: String,
        multisig_wallet_name: String,
    }
```

```json
{
    "uid": "f394a0063e958aa7f0405360c584f24e",
    "multisig_wallet_name": "MultiWallet"
}
```



## 多签账户转账

type：3

content：

```rust
struct MultisigTransfer {
        account_name: Option<String>,
        account_address: String,
        // 交易方式 0转入 1转出
        transfer_type: i8,
        // 交易状态 1-pending 2-成功 3-失败
        status: i8,
    }
```



```json
{
    "account_name": "账户1",
    "account_address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
    "transfer_type": 1,
    "status": 1
}
```



## 多签账户转账等待签名

type：4

content：

```rust
struct MultisigTransferWaitSignature {
        uid: String,
        multisig_wallet_name: String,
    }
```

```json
{
    "uid": "user456",
    "multisig_wallet_name": "MultiWallet2"
}
```

