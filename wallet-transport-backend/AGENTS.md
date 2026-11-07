# AGENTS.md (wallet-transport-backend)

## 关注范围

- 后端 HTTP client 封装与 DTO；新增接口优先在此封装，避免业务层散落 HTTP 细节。

## 测试底线

- 默认测试不得依赖真实 backend/真实网络；真实环境验证必须按 smoke/live 显式隔离。
- 测试分层、fake/mock 与 smoke/live 规则见 `docs/codex/testing.md`。

## 常用命令

- cargo test -p wallet-transport-backend
