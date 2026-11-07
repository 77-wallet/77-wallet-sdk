# 商户侧 MQTT 推送协议

本文整理服务端通过 MQTT 推送给商户侧 SDK 的事件协议。

这份文档描述的是服务端下发的加密事件信封和解密后的业务数据，
与 [mqtt-sdk-message-structures.md](mqtt-sdk-message-structures.md)
中的 SDK 内部消息结构不同。

## 协议结构

服务端推送消息分为两层：

| 层级 | 说明 |
| --- | --- |
| 外层信封 | MQTT 实际收到的消息，包含 `eventNo`、`eventType`、加密 `data`、签名和密钥密文 |
| 业务数据 | `data` 解密后的 JSON，根据 `bizType` 和 `eventType` 解析为不同业务结构 |

常用 Topic：

| Topic | 用途 |
| --- | --- |
| `/aw/merchant/trans/{clientId}` | 交易订单、交易结果、手续费结果 |
| `/aw/merchant/cmd/{client}` | 地址扩容、激活、更换设备等命令类消息 |

## 目录

- [更新记录](#更新记录)
- [枚举定义](#枚举定义)
- [消息类型速查](#消息类型速查)
- [通用消息信封](#通用消息信封)
- [交易消息](#交易消息)
- [交易结果和其他结果通知](#交易结果和其他结果通知)
- [地址池扩容消息](#地址池扩容消息)
- [激活消息](#激活消息)
- [更换设备消息](#更换设备消息)
- [消息校验](#消息校验)

## 更新记录

| 版本 | 更新时间 | 描述 |
| --- | --- | --- |
| v1.0.0 | 2025/09/12 11:00 | 订单消息添加审核字段 |
| v1.0.1 | 2025/09/12 16:35 | 扩容消息添加数量字段 |
| v1.0.2 | 2025/09/17 13:35 | 新增激活消息 |
| v1.0.3 | 2025/09/18 14:35 | 新增订单继续执行事件 |
| v1.0.4 | 2025/09/27 14:35 | 交易新增校验字段 |
| v1.0.5 | 2025/10/14 14:35 | 新增设备变更消息 |
| v1.0.6 | 2025/10/31 14:35 | 结果消息失败新增类型区分 |
| v1.0.7 | 2025/12/24 10:00 | 归集订单不指定 `to` 地址，区分地址类型 |
| v1.0.8 | 2026/04/16 10:00 | 支持资源质押、代理、回收功能 |
| v1.0.9 | 2026/04/20 16:00 | 新增代理完成任务通知消息 |
| v1.1.0 | 2026/04/29 16:00 | 提币交易新增字段 |
| v1.1.1 | 2026/04/30 10:10 | 结果消息新增费用等字段 |
| v1.1.2 | 2026/05/21 10:10 | 资源委托和回收消息新增权限 ID 等字段 |

## 枚举定义

### 交易类型 `tradeType`

| 值 | 含义 | 本地代码接入状态 |
| --- | --- | --- |
| 1 | 提币 | 已接入 |
| 2 | 归集 | 已接入 |
| 3 | 归集手续费交易 | 已接入 |
| 4 | 平台资源质押 | 已接入 |
| 5 | 归集资源委托 | 已接入 |
| 6 | 归集资源回收 | 已接入 |
| 7 | 提币资源委托 | 已接入 |
| 8 | 提币资源回收 | 已接入 |

### 事件类型 `eventType`

| 值 | 含义 |
| --- | --- |
| 1 | 交易事件 |
| 2 | 交易最终结果 |
| 3 | 地址扩容 |
| 4 | 平台解绑 |
| 5 | 激活钱包 |
| 6 | 交易手续费结果 |
| 7 | 设备变更 |
| 8 | 资源代理结果事件 |

说明：本地代码中保留了 `eventType = 4` 的 `AWM_CMD_UID_UNBIND`
解析分支，但当前业务未使用，商户侧协议不作为正式对接消息记录。

### 审核状态 `audit`

| 值 | 含义 |
| --- | --- |
| 1 | 不需要审核 |
| 2 | 需要审核 |

## 消息类型速查

| 消息 | `bizType` | Topic | `eventType` |
| --- | --- | --- | --- |
| 交易订单 | `AWM_ORDER_TRANS` | `/aw/merchant/trans/{clientId}` | `1` |
| 交易最终结果 | `AWM_ORDER_TRANS_RES` | `/aw/merchant/trans/{clientId}` | `2` |
| 交易手续费结果 | `AWM_CMD_FEE_RES` | `/aw/merchant/trans/{clientId}` | `6` |
| 资源结果 | `AWM_CMD_RSC_RES` | `/aw/merchant/cmd/{client}` | `8` |
| 地址池扩容 | `AWM_CMD_ADDR_EXPAND` | `/aw/merchant/cmd/{client}` | `3` |
| 激活钱包 | `AWM_CMD_ACTIVE` | `/aw/merchant/cmd/{client}` | `5` |
| 更换设备 | `AWM_CMD_DEV_CHANGE` | `/aw/merchant/cmd/{client}` | `7` |

## 通用消息信封

所有推送消息外层结构一致：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `eventNo` | String | 事件编号 |
| `eventType` | Integer | 事件类型 |
| `data` | String | 数据体密文 |
| `time` | long | 时间戳 |
| `sign` | String | 签名 |
| `secret` | String | 密钥密文 |

示例：

```json
{
  "eventNo": "3435345363565",
  "data": "dtadtadtatdatdtatdtatdtatdtatdtatdtatdtatdta",
  "time": 95678924567,
  "sign": "SSSSSSSSSSSSSSSSSS",
  "secret": "XXXXXXXXXXXXXXXXXXX"
}
```

## 交易消息

服务端通过交易 Topic 下发待执行的订单类消息，SDK 解密 `data` 后按
`tradeType` 区分普通交易、资源质押、资源代理和资源回收。

当前本地代码 `AwmOrderTransMsg::check_uid` 会按 `tradeType = 1..8`
分发处理普通交易、资源质押、资源委托和资源回收。

### 交易消息基本信息

| 项 | 值 |
| --- | --- |
| 业务类型 `bizType` | `AWM_ORDER_TRANS` |
| Topic | `/aw/merchant/trans/{clientId}` |
| 事件类型 `eventType` | `1` |

### 归集、提币、手续费订单

解密后的 `data` 字段：

<!-- markdownlint-disable MD013 -->
| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `from` | String | `from` 地址 |
| `to` | String | `to` 地址 |
| `value` | String | 数量 |
| `chain` | String | 链 code |
| `tokenAddr` | String | 合约地址 |
| `tokenCode` | String | 合约 code |
| `tradeNo` | String | 平台交易单号 |
| `tradeType` | String | 交易类型，见 `tradeType` 枚举 |
| `audit` | String | 是否需要审核，可空。`1` 不需要审核，`2` 需要审核 |
| `uid` | String | 钱包 ID |
| `validate` | String | 交易校验值 |
| `riskAddr` | String | 风险地址标记。`0` 默认值，无意义；`1` 正常地址；`2` 风险地址。归集交易表示 `from` 地址是否为风险地址，提币订单表示 `to` 地址是否为风险地址 |
| `outOrderId` | String | 商户平台交易单号，可空 |
| `createTime` | String | 交易申请时间，可空 |
| `clientId` | String | 客户 ID，可空 |
<!-- markdownlint-enable MD013 -->

### 资源质押或解锁订单

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `from` | String | 质押或解锁地址 |
| `value` | String | 质押或解锁的资源 TRX 数量 |
| `chain` | String | 链 code，固定为 `tron` |
| `rscType` | String | 资源类型。`0` BANDWIDTH，`1` ENERGY |
| `stkType` | String | 操作类型。`1` STAKE，`2` UN_STAKE |
| `tradeNo` | String | 平台交易单号 |
| `tradeType` | String | 交易类型，固定为 `4` |
| `uid` | String | 平台钱包 UID |

### 资源代理订单

适用于归集资源委托和提币资源委托。

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `from` | String | 支付资源的地址 |
| `to` | String | 接收资源的地址 |
| `nativeValue` | String | 代理的 TRX 数量 |
| `rscValue` | String | 代理的 TRX 转换成资源的数量 |
| `mode` | String | 代理模式。`1` 使用提币地址代理，`2` 使用授权地址代理 |
| `chain` | String | 链 code，固定为 `tron` |
| `rscType` | String | 资源类型。`0` BANDWIDTH，`1` ENERGY |
| `tradeNo` | String | 平台交易单号 |
| `tradeType` | String | 交易类型，`5` 或 `7` |
| `uid` | String | 平台钱包 UID |
| `permissionId` | String | 使用授权地址代理时的权限 ID，可空 |

### 资源回收订单

适用于归集资源回收和提币资源回收。

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `from` | String | 支付资源的地址 |
| `to` | String | 接收资源的地址 |
| `nativeValue` | String | 待回收的 TRX 数量 |
| `rscValue` | String | 待回收的 TRX 转换成资源的数量 |
| `mode` | String | 回收模式。`1` 使用提币地址代理，`2` 使用授权地址代理 |
| `chain` | String | 链 code，固定为 `tron` |
| `rscType` | String | 资源类型。`0` BANDWIDTH，`1` ENERGY |
| `tradeNo` | String | 平台交易单号 |
| `tradeType` | String | 交易类型，`6` 或 `8` |
| `uid` | String | 平台钱包 UID |
| `permissionId` | String | 使用授权地址代理时的权限 ID，可空 |

## 交易结果和其他结果通知

服务端使用结果通知回传交易、手续费或资源代理执行结果。`eventType`
用于区分结果类型，`bizType` 表示具体业务来源。

### 结果通知基本信息

| 项 | 值 |
| --- | --- |
| 业务类型 `bizType` | `AWM_ORDER_TRANS_RES`、`AWM_CMD_FEE_RES`、`AWM_CMD_RSC_RES` |
| Topic | `/aw/merchant/trans/{clientId}`；资源结果为 `/aw/merchant/cmd/{client}` |
| 事件类型 `eventType` | `2`、`6`、`8` |

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `tradeNo` | String | 平台交易单号 |
| `tradeType` | String | 交易类型，见 `tradeType` 枚举 |
| `status` | boolean | 交易结果。`true` 成功，`false` 失败 |
| `failType` | Integer | 订单失败类型。`0` 默认值，无意义；`1` 交易正常失败；`2` 手续费失败 |
| `uid` | String | 钱包 ID |
| `remark` | String | 备注 |
| `blockNumber` | String | 块高 |
| `txFee` | Object | 链上费用信息 |
| `txFee.nativeFee` | String | 本币费用，可空 |
| `txFee.bandwidth` | String | Tron 带宽，可空 |
| `txFee.energy` | String | Tron 能量，可空 |

## 地址池扩容消息

服务端通过命令 Topic 通知 SDK 扩容地址池。`type` 为 `CHA_BATCH`
时表示批量扩容，`CHA_INDEX` 表示指定索引扩容。

### 地址池扩容基本信息

| 项 | 值 |
| --- | --- |
| 业务类型 `bizType` | `AWM_CMD_ADDR_EXPAND` |
| Topic | `/aw/merchant/cmd/{client}` |

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `type` | String | 扩容类型。`CHA_BATCH` 或 `CHA_INDEX` |
| `chain` | String | 链 code |
| `index` | int | 索引 |
| `uid` | String | 钱包 UID |
| `serialNo` | String | 扩容编号 |
| `number` | String | 扩容数量。`type` 为 `CHA_INDEX` 时值为 `1` |
| `batchId` | String | 批次号，上报地址时需要携带 |

## 激活消息

服务端通过命令 Topic 同步钱包激活状态。

### 激活消息基本信息

| 项 | 值 |
| --- | --- |
| 业务类型 `bizType` | `AWM_CMD_ACTIVE` |
| Topic | `/aw/merchant/cmd/{client}` |

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `chain` | String | 链 code |
| `uid` | String | 钱包 UID |
| `active` | int | 激活状态。`0` 未激活，`1` 已激活 |

## 更换设备消息

服务端通过命令 Topic 通知 SDK 钱包绑定设备发生变更。

### 更换设备基本信息

| 项 | 值 |
| --- | --- |
| 业务类型 `bizType` | `AWM_CMD_DEV_CHANGE` |
| Topic | `/aw/merchant/cmd/{client}` |

解密后的 `data` 字段：

| 字段名 | 类型 | 描述 |
| --- | --- | --- |
| `newSn` | String | 新设备 SN |
| `uid` | String | 收款钱包 UID |

## 消息校验

### 交易消息校验

SDK 收到交易消息后，需要校验消息是否被篡改。

校验方法：

1. 取交易中的 `from`、`to`、`value`、`sn`。
2. 按顺序拼接后计算 MD5。
3. 将计算结果与交易中的 `validate` 字段对比。
4. 两者一致时校验成功。

`value` 必须使用不带科学计数法的字符串，并去掉多余的尾随零。
