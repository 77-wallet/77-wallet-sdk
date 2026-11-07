<!-- markdownlint-disable MD013 -->

# 后端接口文档

本文档根据后端 Swagger/OpenAPI 规范整理。

- OpenAPI 版本：`3.1.0`
- 服务地址：由部署环境配置提供
- 默认请求格式：`application/json`
- 常见响应格式：`application/hal+json` 或空响应体

## 接口总览

| 分组 | 方法 | 路径 | 说明 | 操作 ID | 请求结构 | 响应结构 |
| --- | --- | --- | --- | --- | --- | --- |
| 交易 | POST | `/aw/trans/serviceFeeTrans` | 上报打手续费 | `payTransServiceFee` | `ServiceFeeTransReq` | 无 |
| 交易 | POST | `/aw/trans/resourceDl/apply` | 资源委托申请申请 | `applyResource` | `ResourceApplyReq` | `ApplyResourceDlRep` |
| 交易 | POST | `/aw/trans/executeComplete` | 上报执行结果 | `transComplete` | `TransCompleteReq` | 无 |
| 交易 | POST | `/aw/trans/eventAck` | 收到交易事件确认 | `ackTrans` | `EventAckReq` | 无 |
| 交易 | POST | `/aw/trans/cancel` | 取消交易 | `cancel` | `TransCancelReq` | 无 |
| 交易 | POST | `/aw/trans/audit` | 审核交易 | `auditTrans` | `AuditResultReq` | 无 |
| 代币信息 | POST | `/aw/token/queryByPage` | 代币(分页查询) | `queryByPage` | `AwTokenPageReq` | `object` |
| 策略管理 | POST | `/aw/strategy/withdrawal/save` | 提币策略保存 | `saveWithdrawalStrategy` | `StrategyConfigReq` | 无 |
| 策略管理 | POST | `/aw/strategy/getWithdrawalConfig` | 提币策略查询 | `queryWithdrawalStrategy` | `QueryReq` | `StrategyConfigResp` |
| 策略管理 | POST | `/aw/strategy/getCollectConfig` | 归集策略获取 | `queryCollectStrategy` | `QueryReq` | `StrategyConfigResp` |
| 策略管理 | POST | `/aw/strategy/collect/save` | 归集策略保存 | `saveCollectStrategy` | `StrategyConfigReq` | 无 |
| API钱包消息 | POST | `/aw/msg/ack` | API钱包消息确认 | `msgAck` | `array<MsgAckReq>` | 无 |
| API钱包消息 | POST | `/aw/msg/ackExpired/resend` | 超时未确认消息重新推送 | `timeoutMsgResend` | `ClientReq` | 无 |
| 通信密钥 | POST | `/aw/init/swap` | 密钥初始交换 | `initSecret` | `InitSecret` | `SecretRep` |
| 通信密钥 | POST | `/aw/init/apiWallet` | 设置UID为API钱包 | `markUid` | `UidReq` | 无 |
| 链信息 | POST | `/aw/chain/list` | 查询链列表 | `chainList` | `AwChainReq` | `array<AwChainResp>` |
| UID与APPID的绑定 | POST | `/aw/appid/wdWallet/change` | 更换出款钱包 | `updateWithdrawalWallet` | `UpdateWithdrawalWalletReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/unbind` | UID与APPID的解绑 | `unbind` | `UnbindReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/uid/usage` | 查询钱包在uid下的使用状态 | `appIdIncludeUidInfo` | `AppIdUidReq` | `AppIdUidResp` |
| UID与APPID的绑定 | POST | `/aw/appid/rechargeWallet/import` | 导入收款钱包 | `importRechargeWallet` | `ImportRechargeWalletReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/rechargeWallet/bind` | 绑定收款钱包 | `bindRechargeWallet` | `BindRechargeWalletReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/import` | 导入钱包 | `importWallet` | `ImportWalletReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/getActiveInfo` | 查询钱包激活信息 | `walletActiveInfo` | `BindUidReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/configs` | 查询配置 | `getDefaultConfigs` | 无 | `PlatformConfigsResp` |
| UID与APPID的绑定 | POST | `/aw/appid/bind` | UID绑定appId | `bind` | `BindReq` | 无 |
| UID与APPID的绑定 | POST | `/aw/appid/bindInfo` | 查询uid 绑定信息 | `getBindInfo` | `BindUidReq` | 无 |
| 地址管理 | POST | `/aw/address/list` | 查询uid的地址列表 | `getAddressList` | `UidAddressReq` | `object` |
| 地址管理 | POST | `/aw/address/init` | 地址初始化 | `addressInit` | `AddressInitReq` | 无 |
| 地址管理 | POST | `/aw/address/expand/complete` | 扩容完成上报 | `expandComplete` | `ExpandCompleteReq` | 无 |
| 地址管理 | POST | `/aw/address/assetList` | 查询链下index的资产 | `getIndexAssets` | `IndexAssetsReq` | `array<IndexAssetsRep>` |

## 调用说明

### 交易

#### 上报打手续费

- 方法：`POST`
- 路径：`/aw/trans/serviceFeeTrans`
- 请求体：`ServiceFeeTransReq`
- 响应：`200 OK`

#### 资源委托申请申请

- 方法：`POST`
- 路径：`/aw/trans/resourceDl/apply`
- 请求参数：`ResourceApplyReq`
- 响应：`ApplyResourceDlRep`

#### 上报执行结果

- 方法：`POST`
- 路径：`/aw/trans/executeComplete`
- 请求体：`TransCompleteReq`
- 响应：`200 OK`

#### 收到交易事件确认

- 方法：`POST`
- 路径：`/aw/trans/eventAck`
- 请求体：`EventAckReq`
- 响应：`200 OK`

#### 取消交易

- 方法：`POST`
- 路径：`/aw/trans/cancel`
- 请求体：`TransCancelReq`
- 响应：`200 OK`

#### 审核交易

- 方法：`POST`
- 路径：`/aw/trans/audit`
- 请求体：`AuditResultReq`
- 响应：`200 OK`

### 代币信息

#### 代币分页查询

- 方法：`POST`
- 路径：`/aw/token/queryByPage`
- 请求体：`AwTokenPageReq`
- 响应：`object`

### 策略管理

#### 提币策略保存

- 方法：`POST`
- 路径：`/aw/strategy/withdrawal/save`
- 请求体：`StrategyConfigReq`
- 响应：`200 OK`

#### 提币策略查询

- 方法：`POST`
- 路径：`/aw/strategy/getWithdrawalConfig`
- 请求体：`QueryReq`
- 响应：`StrategyConfigResp`

#### 归集策略获取

- 方法：`POST`
- 路径：`/aw/strategy/getCollectConfig`
- 请求体：`QueryReq`
- 响应：`StrategyConfigResp`

#### 归集策略保存

- 方法：`POST`
- 路径：`/aw/strategy/collect/save`
- 请求体：`StrategyConfigReq`
- 响应：`200 OK`

### API钱包消息

#### API钱包消息确认

- 方法：`POST`
- 路径：`/aw/msg/ack`
- 请求体：`array<MsgAckReq>`
- 响应：`200 OK`

#### 超时未确认消息重新推送

- 方法：`POST`
- 路径：`/aw/msg/ackExpired/resend`
- 请求体：`ClientReq`
- 响应：`200 OK`

### 通信密钥

#### 密钥初始交换

- 方法：`POST`
- 路径：`/aw/init/swap`
- 请求体：`InitSecret`
- 响应：`SecretRep`

#### 设置UID为API钱包

- 方法：`POST`
- 路径：`/aw/init/apiWallet`
- 请求体：`UidReq`
- 响应：`200 OK`

### 链信息

#### 查询链列表

- 方法：`POST`
- 路径：`/aw/chain/list`
- 请求体：`AwChainReq`
- 响应：`array<AwChainResp>`

### UID与APPID的绑定

#### 更换出款钱包

- 方法：`POST`
- 路径：`/aw/appid/wdWallet/change`
- 请求体：`UpdateWithdrawalWalletReq`
- 响应：`200 OK`

#### UID与APPID的解绑

- 方法：`POST`
- 路径：`/aw/appid/unbind`
- 请求体：`UnbindReq`
- 响应：`200 OK`

#### 查询钱包在uid下的使用状态

- 方法：`POST`
- 路径：`/aw/appid/uid/usage`
- 请求体：`AppIdUidReq`
- 响应：`AppIdUidResp`

#### 导入收款钱包

- 方法：`POST`
- 路径：`/aw/appid/rechargeWallet/import`
- 请求体：`ImportRechargeWalletReq`
- 响应：`200 OK`

#### 绑定收款钱包

- 方法：`POST`
- 路径：`/aw/appid/rechargeWallet/bind`
- 请求体：`BindRechargeWalletReq`
- 响应：`200 OK`

#### 导入钱包

- 方法：`POST`
- 路径：`/aw/appid/import`
- 请求体：`ImportWalletReq`
- 响应：`200 OK`

#### 查询钱包激活信息

- 方法：`POST`
- 路径：`/aw/appid/getActiveInfo`
- 请求体：`BindUidReq`
- 响应：`200 OK`

#### 查询配置

- 方法：`POST`
- 路径：`/aw/appid/configs`
- 请求体：无
- 响应：`PlatformConfigsResp`

#### UID绑定appId

- 方法：`POST`
- 路径：`/aw/appid/bind`
- 请求体：`BindReq`
- 响应：`200 OK`

#### 查询uid绑定信息

- 方法：`POST`
- 路径：`/aw/appid/bindInfo`
- 请求体：`BindUidReq`
- 响应：`200 OK`

### 地址管理

#### 查询uid的地址列表

- 方法：`POST`
- 路径：`/aw/address/list`
- 请求体：`UidAddressReq`
- 响应：`object`

#### 地址初始化

- 方法：`POST`
- 路径：`/aw/address/init`
- 请求体：`AddressInitReq`
- 响应：`200 OK`

#### 扩容完成上报

- 方法：`POST`
- 路径：`/aw/address/expand/complete`
- 请求体：`ExpandCompleteReq`
- 响应：`200 OK`

#### 查询链下index的资产

- 方法：`POST`
- 路径：`/aw/address/assetList`
- 请求体：`IndexAssetsReq`
- 响应：`array<IndexAssetsRep>`

## 数据结构

### ServiceFeeTransReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tradeNo` | `string` | 是 | 平台交易单号 |
| `from` | `string` | 是 | from 地址 |
| `to` | `string` | 是 | to 地址 |
| `amount` | `number` | 是 | 交易金额 |
| `chainCode` | `string` | 是 | 链编码 |
| `tokenCode` | `string` | 是 | 代币简码 |
| `contractAddress` | `string` | 否 | 代币合约地址 |

### ResourceApplyReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tradeNo` | `string` | 是 | 平台交易单号,归集单或提币单 |
| `appId` | `string` | 是 | 应用appId |
| `orgId` | `string` | 是 | 商户Id |
| `chain` | `string` | 否 | 链编码 |
| `nativeTokenAmount` | `number` | 是 | 申请的资源换算成本币的数量 |
| `resourceAmount` | `number` | 否 | 代理的资源数量 |
| `resourceType` | `string` | 是 | 申请代理的资源类型 |
| `to` | `string` | 是 | 接收资源的地址 |
| `type` | `string` | 是 | 交易类型 |

枚举说明：

- `resourceType`：`BANDWIDTH` / `ENERGY`
- `type`：`COL` / `WD` / `COL_FEE` / `PLT_RSC_STK` / `COL_RSC_DL` / `COL_RSC_RC` / `WD_RSC_DL` / `WD_RSC_RC`

### ApplyResourceDlRep

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `dlRes` | `boolean` | 否 | 申请结果 |
| `dlTradeNo` | `string` | 否 | 资源单号 |

### TransCompleteReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tradeNo` | `string` | 是 | 平台交易单号 |
| `type` | `string` | 否 | 交易类型 |
| `hash` | `string` | 否 | 链上哈希 |
| `status` | `string` | 否 | 执行结果 |
| `errorCode` | `string` | 否 | 错误码 |
| `remark` | `string` | 否 | 备注 |
| `from` | `string` | 否 | 本次交易上链时的 from 地址 |
| `to` | `string` | 否 | 本次交易上链时的 to 地址 |

枚举说明：

- `type`：`COL` / `WD` / `COL_FEE` / `PLT_RSC_STK` / `COL_RSC_DL` / `COL_RSC_RC` / `WD_RSC_DL` / `WD_RSC_RC`
- `status`：`SUCCESS` / `FAIL` / `FAIL_RETRY`
- `errorCode`：`ERR_6001` / `ERR_6002` / `ERR_6003` / `ERR_6004` / `ERR_6005` / `ERR_6006` / `ERR_6008` / `ERR_6099`

### EventAckReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tradeNo` | `string` | 是 | 交易单号 |
| `type` | `string` | 否 | 交易类型 |
| `ackType` | `string` | 否 | 确认类型类型 |

枚举说明：

- `type`：`COL` / `WD` / `COL_FEE` / `PLT_RSC_STK` / `COL_RSC_DL` / `COL_RSC_RC` / `WD_RSC_DL` / `WD_RSC_RC`
- `ackType`：`TX` / `TX_RES` / `CMD_ADDRESS_EXPAND` / `CMD_PLT_UID_UNBIND` / `CMD_WALLET_ACTIVE` / `TX_FEE_RES` / `DEV_CHANGE` / `TX_RSC_RES`

### TransCancelReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tradeNo` | `string` | 是 | 平台交易单号 |
| `type` | `string` | 否 | 交易类型 |
| `remark` | `string` | 否 | 备注 |

枚举说明：

- `type`：`COL` / `WD` / `COL_FEE` / `PLT_RSC_STK` / `COL_RSC_DL` / `COL_RSC_RC` / `WD_RSC_DL` / `WD_RSC_RC`

### AuditResultReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tradeNo` | `string` | 是 | 平台交易单号 |
| `result` | `boolean` | 是 | 审核结果： true 通过 / false 拒绝 |
| `remark` | `string` | 否 | 备注 |

### AwTokenPageReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `orderColumn` | `string` | 否 | 排序，示例：`create_time` |
| `orderType` | `string` | 否 | 排序类型：`ASC` / `DESC`，默认 `DESC` |
| `chainCode` | `string` | 否 | 链编码 |
| `code` | `string` | 否 | 代币编码 |
| `createTime` | `string` | 否 | 创建时间 |
| `updateTime` | `string` | 否 | 更新时间 |
| `pageNum` | `integer(int32)` | 否 | 页索引 |
| `pageSize` | `integer(int32)` | 否 | 页大小，默认 `10` |

### AddressItem

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `index` | `integer(int32)` | 否 | 地址下标 |
| `address` | `string` | 是 | 地址 |

### ChainAddress

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `chainCode` | `string` | 是 | 链编码 |
| `normalAddress` | `object` | 是 | 正常地址 |
| `riskAddress` | `object` | 是 | 风险地址 |
| `chainAddressType` | `string` | 否 | 链上地址类型 |

### StrategyConfigReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `uid` | `string` | 是 | uid |
| `threshold` | `number` | 是 | 阈值 |
| `chainConfigs` | `array<ChainAddress>` | 是 | 链地址配置 |

### QueryReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `uid` | `string` | 是 | uid |

### StrategyConfigResp

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `uid` | `string` | 是 | uid |
| `threshold` | `number` | 是 | 阈值 |
| `chainConfigs` | `array<ChainAddress>` | 是 | 链地址配置 |

### MsgAckReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `msgId` | `string` | 是 | 消息ID |

### ClientReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `clientId` | `string` | 是 | clientID |

### InitSecret

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `sn` | `string` | 是 | 设备sn |
| `clientPubKey` | `string` | 是 | 客户端公钥：Base64编码。x.509 PEM 格式 |

### SecretRep

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `pubKey` | `string` | 否 | 服务端公钥: X.509 PEM 格式 |

### UidReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `sn` | `string` | 是 | 设备sn |
| `rechargeUid` | `string` | 否 | 收款uid |
| `withdrawalUid` | `string` | 否 | 提现uid |

### AwChainReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `appVersionCode` | `string` | 是 | APP版本号 |

### AwChainResp

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `id` | `string` | 否 | id |
| `appVersionCode` | `string` | 否 | APP版本号 |
| `name` | `string` | 否 | 链名称 |
| `code` | `string` | 否 | code |
| `chainId` | `integer(int32)` | 否 | 链id |
| `globalHeight` | `integer(int64)` | 否 | 线上块高 |
| `localHeight` | `integer(int64)` | 否 | 本地块高 |
| `enable` | `boolean` | 否 | 启用 |
| `seq` | `integer(int32)` | 否 | 排序 |
| `defaultChain` | `boolean` | 否 | 默认链 |
| `tgAlert` | `boolean` | 否 | TG监控通知 |
| `addressUrl` | `string` | 否 | 查看链上地址URL |
| `hashUrl` | `string` | 否 | 查看链上hash URL |
| `tokenUrl` | `string` | 否 | 查看链上合约地址URL |
| `enableBlock` | `boolean` | 否 | 启用块高监听开关 |
| `blockTime` | `string` | 否 | 出块时间 例：2秒，1小时，针对tg |
| `createTime` | `string` | 否 | 创建时间 |
| `updateTime` | `string` | 否 | 更新时间 |
| `masterTokenCode` | `string` | 否 | 主币编码 |

### UpdateWithdrawalWalletReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `withdrawalUid` | `string` | 是 | 新的出款钱包uid |
| `orgAppId` | `string` | 是 | appId |

### UnbindReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `rechargeUid` | `string` | 是 | 子账户UID |
| `withdrawalUid` | `string` | 是 | 出款账户UID |
| `orgAppId` | `string` | 是 | 商户appId |

### AppIdUidReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `orgAppId` | `string` | 是 | appId |
| `uid` | `string` | 是 | 钱包uid |
| `walletType` | `string` | 是 | 钱包类型 |

枚举说明：

- `walletType`：`NORMAL_WALLET`（普通钱包） / `API_RAW`（API钱包-收款钱包） / `API_WAW`（API钱包-出款钱包） / `NOT_FOUND`（UID不存在）

### AppIdUidResp

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `used` | `boolean` | 否 | uid是否在appId下使用过 |

### ImportRechargeWalletReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `sn` | `string` | 是 | 设备sn |
| `rechargeUid` | `string` | 是 | 子账户UID |

### BindRechargeWalletReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `rechargeUid` | `string` | 是 | 子账户UID |
| `orgAppId` | `string` | 是 | 商户appId |
| `sn` | `string` | 是 | sn |

### ImportWalletReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `sn` | `string` | 是 | 设备sn |
| `withdrawalUid` | `string` | 是 | 出款账户UID |
| `rechargeUid` | `string` | 是 | 子账户UID |

### BindUidReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `uid` | `string` | 是 | uid |

### PlatformConfigsResp

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `configs` | `object` | 否 | 配置键值 |

### BindReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `rechargeUid` | `string` | 是 | 子账户UID |
| `withdrawalUid` | `string` | 是 | 出款账户UID |
| `orgAppId` | `string` | 是 | 商户appId |
| `sn` | `string` | 是 | sn |

### UidAddressReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `orderColumn` | `string` | 否 | 排序，示例：`create_time` |
| `orderType` | `string` | 否 | 排序类型：`ASC` / `DESC`，默认 `DESC` |
| `uid` | `string` | 是 | uid。该uid只能是子账户uid |
| `chainCode` | `string` | 是 | 链编码 |
| `pageNum` | `integer(int32)` | 否 | 页索引 |
| `pageSize` | `integer(int32)` | 否 | 页大小，默认 `10` |

### AddressInitReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `addressList` | `array<KeyAddressDto>` | 是 | 地址信息 |
| `batchId` | `string` | 否 | 扩容批次号，可为空 |

### KeyAddressDto

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `uid` | `string` | 是 | 助记词id |
| `address` | `string` | 是 | 地址 |
| `index` | `integer(int32)` | 是 | 地址下标 |
| `chainCode` | `string` | 是 | 链编码 |
| `sn` | `string` | 是 | 设备号 |
| `contractAddress` | `array<string>` | 否 | 合约地址 |
| `name` | `string` | 否 | 账户名称 |

### ExpandCompleteReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `uid` | `string` | 是 | 钱包id |
| `serialNo` | `string` | 是 | 扩容事件no |
| `status` | `boolean` | 是 | 处理结果 |
| `remark` | `string` | 否 | 备注 |
| `batchId` | `string` | 否 | 扩容批次号，可为空 |

### IndexAssetsReq

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `chainCode` | `string` | 是 | 链编码 |
| `uid` | `string` | 是 | uid。该uid只能是子账户uid |
| `indexList` | `array<integer(int32)>` | 否 | index |

### AddressAssetsRep

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `address` | `string` | 否 | 地址 |
| `tokenInfos` | `array<AddressTokenInfoRep>` | 否 | 地址的代币信息 |

### AddressTokenInfoRep

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `tokenCode` | `string` | 否 | 代币code |
| `tokenAddress` | `string` | 否 | 代币地址 |
| `amount` | `number` | 否 | 代币数量 |
| `assets` | `number` | 否 | 代币资产 |

### IndexAssetsRep

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `index` | `integer(int32)` | 否 | 地址下标 |
| `addressList` | `array<AddressAssetsRep>` | 否 | 地址列表 |
