# ECDH 与 Backend 集成说明

## Purpose

本文描述的是当前工程里的集成实现：`wallet-ecdh` 如何为 `wallet-api` + `wallet-transport-backend` 提供会话态密钥交换、请求加密签名、响应验签解密。  
这不是协议设计提案，也不讨论“理想密码学重构”。

## Core Components

- `wallet-ecdh::GLOBAL_KEY`  
  进程级会话状态入口，保存 `sn` 与 `shared_secret`。调用方需要先完成握手，后续 API 才能加密与验签。
- `wallet-ecdh::ExKey`  
  提供 `secret_pub_key / set_shared_secret / is_exchange_shared_secret / encrypt / sign / verify / decrypt`。
- `wallet-transport-backend::api_request::ApiBackendRequest`  
  请求出站封装：`serialize -> encrypt -> sign`，并携带 `sn`。
- `wallet-transport-backend::response::api_response::ApiBackendResponse`  
  响应入站处理：`verify -> decrypt -> deserialize`。

## End-to-End Flow

### 1) 握手阶段（wallet-api 发起）

真实调用点：

- `wallet-api/src/service/api_wallet/wallet.rs`
  - `GLOBAL_KEY.secret_pub_key()` 生成客户端公钥
  - 调后端 `init_swap`
  - 使用返回 `pub_key` 执行 `GLOBAL_KEY.set_shared_secret(...)`

```mermaid
sequenceDiagram
    participant API as wallet-api
    participant ECDH as wallet-ecdh::GLOBAL_KEY
    participant BE as backend(init_swap)

    API->>ECDH: secret_pub_key()
    API->>BE: init_swap(sn, client_pub_key)
    BE-->>API: server_pub_key
    API->>ECDH: set_shared_secret(server_pub_key)
    API->>ECDH: is_exchange_shared_secret() == Ok
```

### 2) 请求出站（transport backend）

真实调用点：

- `wallet-transport-backend/src/api_request.rs`
  - `GLOBAL_KEY.encrypt(req_json_bytes)`
  - `GLOBAL_KEY.sign(tag, key+ciphertext)`
  - `sn = GLOBAL_KEY.sn()`

```mermaid
flowchart LR
    A[Request DTO] --> B[serialize to json]
    B --> C[GLOBAL_KEY.encrypt]
    C --> D[build body key + ciphertext]
    D --> E[GLOBAL_KEY.sign]
    E --> F[ApiBackendRequest sn/sign/body]
    F --> G[transport send]
```

### 3) 响应入站（transport backend）

真实调用点：

- `wallet-transport-backend/src/response/api_response.rs`
  - `GLOBAL_KEY.verify(tag, key+data, sign)`
  - `GLOBAL_KEY.decrypt(data, key)`
  - `serde_from_slice(...)`

```mermaid
flowchart LR
    A[Backend response payload] --> B[decode sign/key/data]
    B --> C[GLOBAL_KEY.verify]
    C --> D[GLOBAL_KEY.decrypt]
    D --> E[deserialize business object]
    E --> F[return typed response]
```

## State Model

`GLOBAL_KEY` 是进程级状态，核心字段是：

- `sn`：由上层在启动/初始化阶段写入（例如 manager 初始化）
- `shared_secret`：在 `init_swap` 成功后写入，后续所有加密签名依赖它

客户端 ECDH 私钥在 `ExKey` 创建时由 `OsRng` 随机生成。生产入口
`GLOBAL_KEY` 在进程首次使用时创建，后续握手和 MQTT 重连复用该私钥；
新进程生成新密钥，必须重新握手。私钥不从随包配置读取，也不持久化。
请求/响应格式、共享密钥派生和消息加密验签格式保持不变。

读取方：

- `wallet-transport-backend` 请求封装与响应解包
- 各 API 方法前置检查 `GLOBAL_KEY.is_exchange_shared_secret()`

```mermaid
stateDiagram-v2
    [*] --> NoSharedSecret
    NoSharedSecret --> SharedReady: set_shared_secret(ok)
    SharedReady --> InvalidOrReset: reset/error/process restart
    InvalidOrReset --> SharedReady: set_shared_secret(ok)
```

## Failure Modes

- 未握手：`is_exchange_shared_secret()` 返回 `InvalidSharedKey`，API 调用前置失败。
- 请求签名失败：`GLOBAL_KEY.sign(...)` 返回签名相关错误。
- 响应验签失败：`GLOBAL_KEY.verify(...)` 返回 `InvalidSignature` 或签名校验错误。
- 响应解密失败：`GLOBAL_KEY.decrypt(...)` 返回解密失败错误。
- 全局状态污染：并发测试或跨用例共享 `GLOBAL_KEY` 导致状态串扰（已在 `wallet-ecdh` 提供 test-only reset 辅助）。

## Testing & Validation

建议离线最小验证：

```bash
cargo test -p wallet-ecdh
cargo test -p wallet-transport-backend --lib
```

验收重点：

- 握手成功后，出站请求可完成 `encrypt + sign`
- 入站响应可完成 `verify + decrypt`
- 未握手路径会被 `InvalidSharedKey` 拦截
