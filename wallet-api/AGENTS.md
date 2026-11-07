# AGENTS.md (wallet-api)

## 关注范围
- 业务编排层（WalletManager/API/service/domain），避免散落 HTTP 细节。

## 测试底线
- 修改业务流程（多签/质押/交易/归集/提币）必须补/更关键路径集成测试：成功 / 回退或恢复 / 失败
- 修复缺陷必须补可复现该缺陷的回归用例
- 测试分层、fixture、断言矩阵与 smoke/live 隔离规则见 `docs/codex/testing.md`

## 常用命令
- cargo test -p wallet-api
