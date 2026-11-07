# AGENTS.md (wallet-database)

## 关注范围
- SQLite 数据层（entities/repositories/migrations）。

## 测试底线
- 修改 schema/migrations：同步更新实体与仓储逻辑，并验证迁移可执行
- 影响 DB 读写/仓储查询/事务边界：必须补 SQLite 集成测试并断言真实落库结果
- 修复缺陷必须补回归用例
- 测试分层、临时 SQLite fixture 与断言规则见 `docs/codex/testing.md`

## 常用命令
- cargo test -p wallet-database
