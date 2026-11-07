# CI Release 自动化规则

本文档描述 CI 如何自动执行检查、生成 nightly 版本、发布 stable 版本。

## 1. CI 触发规则

### Pull Request

触发条件：

```text
feature/* -> develop
fix/* -> develop
codex/* -> develop
```

执行：

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

要求：

- 全部通过才能合并
- 数据库 migration 修改必须人工 review
- FFI 接口修改必须人工 review

---

## 2. develop 合并后

当代码合并到 develop 后，CI 自动生成 nightly。

tag 格式：

```text
nightly-YYYYMMDD
```

示例：

```text
nightly-20260501
```

流程：

```bash
NIGHTLY_TAG=nightly-$(date +%Y%m%d)
git checkout develop
git pull
git tag "$NIGHTLY_TAG"
git push origin "$NIGHTLY_TAG"
```

注意：

- 如果当天已经存在 nightly tag，不要覆盖
- 可以改成 `nightly-YYYYMMDD.N`

示例：

```text
nightly-20260501.1
nightly-20260501.2
```

---

## 3. release 分支

当创建 release 分支时：

```text
release/2.1.0
```

CI 执行完整测试：

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

release 分支只允许：

- 修 bug
- 改版本号
- 更新 changelog（如仓库维护 changelog）
- 补文档

不允许：

- 新功能开发

---

## 4. main 合并后

当 release 分支合并到 main 后，CI 不自动发布 stable。

stable 版本必须通过人工确认后打 tag：

```bash
git tag v2.1.0
git push origin v2.1.0
```

原因：

- stable 是正式版本
- 需要人工确认 changelog、版本号、测试结果

---

## 5. stable tag 发布

当检测到 tag：

```text
v*
```

例如：

```text
v2.1.0
```

CI 执行：

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

然后生成发布产物。

---

## 6. Hotfix 自动化

hotfix 分支命名：

```text
hotfix/2.1.1-tron-balance
```

来源：

```text
main
```

CI 执行：

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

合并流程：

```text
hotfix/* -> main
hotfix/* -> develop
```

打 tag：

```text
v2.1.1
```

---

## 7. CHANGELOG 自动化

推荐使用 Conventional Commits。

示例：

```text
feat: add tron staking
fix: filter repeated token
docs: update branching rules
refactor: split wallet service
test: add sqlite migration tests
```

CHANGELOG 分类：

```text
feat     -> Features
fix      -> Fixes
docs     -> Documentation
refactor -> Refactoring
test     -> Tests
```

---

## 8. 推荐 CI 流程图

```text
feature/* / fix/*
        ↓
      PR
        ↓
  fmt/check/test
        ↓
    develop
        ↓
 nightly tag
        ↓
 release/x.x.x
        ↓
 full test
        ↓
      main
        ↓
    tag vX.X.X
        ↓
 stable release
```

---

## 9. 最终规则

- PR 必须跑基础检查
- develop 合并后生成 nightly
- release 分支跑完整测试
- main 只保存可发布代码
- stable 必须通过 tag 发布
- tag 不允许覆盖
- SDK 使用方必须优先依赖 tag
