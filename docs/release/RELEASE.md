# RELEASE.md

## 发布流程

本文档描述项目的版本发布、nightly 构建以及 hotfix 修复流程。

核心原则：

> branch 表达开发流程
> tag 表达发布版本
> 发布版本必须来自 main

---

## 1. 版本类型

项目包含两种版本：

```text
nightly
stable
```

### nightly

测试版本。

特点：

- 自动生成
- 不保证稳定
- 用于内部测试 / 联调

格式：

```text
nightly-YYYYMMDD
```

示例：

```text
nightly-20260501
nightly-20260502
```

来源：

```text
develop
```

---

### stable

正式发布版本。

特点：

- 经过测试
- 用于生产环境
- SDK 使用方推荐依赖

格式：

```text
v<major>.<minor>.<patch>
```

示例：

```text
v2.1.0
v2.1.1
v2.2.0
```

来源：

```text
main
```

---

## 2. 发版流程（Stable）

### Step 1 创建 release 分支

从 develop 创建 release 分支。

```bash
git checkout develop
git pull

git checkout -b release/2.1.0
```

---

### Step 2 准备发布

release 分支只允许：

- bug 修复
- 文档更新
- changelog 更新
- 版本号更新

禁止：

- 新功能开发

更新 Rust SDK 版本号：

```text
Cargo.toml
```

更新发布说明：

```text
CHANGELOG.md（如仓库维护 changelog）
```

---

### Step 3 测试验证

release 分支必须通过：

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

同时验证：

- 数据库 migration
- RPC 接口
- MQTT 连接
- 钱包导入
- 资产同步

---

### Step 4 合并到 main

测试完成后：

```bash
git checkout main
git pull

git merge release/2.1.0
```

---

### Step 5 打 tag

创建发布版本：

```bash
git tag v2.1.0
git push origin v2.1.0
```

---

### Step 6 同步 develop

release 上的修复必须同步回 develop。

```bash
git checkout develop
git merge release/2.1.0
```

---

### Step 7 删除 release 分支

```bash
git branch -d release/2.1.0
git push origin --delete release/2.1.0
```

---

## 3. Nightly 版本

nightly 用于内部测试。

CI 可以每天自动生成。

生成方式：

```bash
NIGHTLY_TAG=nightly-$(date +%Y%m%d)
git tag "$NIGHTLY_TAG"
git push origin "$NIGHTLY_TAG"
```

如果当天 tag 已存在，使用递增后缀：

```text
nightly-YYYYMMDD.N
```

示例：

```text
nightly-20260501
nightly-20260502
```

来源：

```text
develop
```

---

## 4. Hotfix 发布

如果 stable 版本出现严重问题：

从 main 创建 hotfix 分支。

```bash
git checkout main
git checkout -b hotfix/2.1.1-fix-tron-balance
```

修复后：

```bash
git checkout main
git merge hotfix/2.1.1-fix-tron-balance
```

打 tag：

```bash
git tag v2.1.1
git push origin v2.1.1
```

同步 develop：

```bash
git checkout develop
git merge hotfix/2.1.1-fix-tron-balance
```

删除 hotfix 分支：

```bash
git branch -d hotfix/2.1.1-fix-tron-balance
```

---

## 5. SDK 依赖规则

推荐：

```yaml
wallet_sdk:
  git:
    url: xxx
    ref: v2.1.0
```

测试版本：

```yaml
wallet_sdk:
  git:
    url: xxx
    ref: nightly-20260501
```

禁止：

```yaml
ref: main
ref: develop
ref: stable
```

原因：

- branch 会变化
- tag 永远指向同一代码

---

## 6. CHANGELOG 规则

每次 stable 发布必须更新：

```text
CHANGELOG.md（如仓库维护 changelog）
```

示例：

```text
## v2.1.0

Features
- Add Tron staking support
- Improve MQTT reconnect logic

Fixes
- Fix token repeat filtering
- Fix sqlite chain status update
```

---

## 7. CI 推荐流程

CI 可以自动执行：

```text
PR -> run tests
merge develop -> nightly build
tag vX.X.X -> stable release
```

推荐自动化：

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

---

## 8. 发布检查清单

发布 stable 前必须确认：

- 所有 CI 通过
- migration 兼容
- 钱包导入测试通过
- RPC 节点连接正常
- 资产同步正常
- 多链功能正常
- changelog 已更新
- 版本号已更新

---

## 9. 最终规则

项目发布结构：

```text
branch

main
develop
feature/*
fix/*
release/*
hotfix/*
codex/*
experiment/*
```

版本：

```text
tag

v2.1.0
v2.1.1
nightly-20260501
nightly-20260502
```

最终原则：

> 所有正式发布版本必须通过 tag 管理。
