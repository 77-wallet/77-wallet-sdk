# BRANCHING.md

## Git 分支与发布规则

本项目使用统一的 Git 分支和版本管理规则，以保证：

- 开发流程清晰
- 版本可追溯
- SDK 可稳定依赖
- CI/CD 自动化顺畅

核心原则：

> branch 表达开发流程
> tag 表达发布版本
> 环境通过配置区分，不通过分支区分

---

## 1. 长期分支

仓库只允许以下长期分支：

```text
main
develop
```

### main

稳定发布分支。

规则：

- main 必须始终保持可构建
- main 上代码必须经过测试
- 所有正式版本从 main 打 tag
- SDK 使用方应依赖 main 上的 tag

示例：

```text
v2.1.0
v2.1.1
v2.2.0
```

---

### develop

开发集成分支。

规则：

- 所有 feature/fix 分支先合入 develop
- develop 必须通过基本 CI
- develop 可以包含未发布功能

---

## 2. 临时分支

所有开发必须使用临时分支。

### feature 分支

用于开发新功能。

命名规则：

```text
feature/<short-description>
```

示例：

```text
feature/tron-stake
feature/mqtt-reconnect
feature/import-sub-wallet
feature/token-cache
```

合并方向：

```text
feature/* → develop
```

---

### fix 分支

用于普通 bug 修复。

命名规则：

```text
fix/<short-description>
```

示例：

```text
fix/token-repeat-filter
fix/sqlite-chain-status
fix/mqtt-auth
```

合并方向：

```text
fix/* → develop
```

---

### hotfix 分支

用于线上紧急问题修复。

命名规则：

```text
hotfix/<version>-<description>
```

示例：

```text
hotfix/2.1.1-tron-balance
hotfix/2.1.2-keystore-v3
```

流程：

```text
main → hotfix/*
hotfix/* → main
hotfix/* → develop
```

注意：

- hotfix 必须从 main 创建
- 修复完成后必须同步回 develop

---

### release 分支

用于准备发布版本。

命名规则：

```text
release/<version>
```

示例：

```text
release/2.1.0
release/2.2.0
```

release 分支只允许：

- bug 修复
- 文档更新
- 版本号更新
- changelog 更新

禁止：

- 新功能开发

发布流程：

```text
develop → release/*
release/* → main
release/* → develop
```

---

### codex 分支

AI / Codex 自动生成代码使用。

命名规则：

```text
codex/<description>
```

示例：

```text
codex/refactor-chain-repo
codex/add-db-tests
codex/fix-mqtt-retry
```

规则：

- 必须经过人工 review
- 合入 develop 后删除

---

### experiment 分支

实验性开发。

命名规则：

```text
experiment/<description>
```

示例：

```text
experiment/uniswap-v3
experiment/new-node-ranking
```

特点：

- 不保证最终合并
- 用于验证新想法

---

## 3. 分支合并规则

正常开发流程：

```text
feature/* → develop
fix/*     → develop
develop   → release/*
release/* → main
release/* → develop
```

紧急修复流程：

```text
main → hotfix/*
hotfix/* → main
hotfix/* → develop
```

---

## 4. 发布规则

正式版本必须使用 Git tag。

版本格式：

```text
v<major>.<minor>.<patch>
```

示例：

```text
v2.1.0
v2.1.1
v2.2.0
```

发布步骤：

1. 从 develop 创建 release 分支

   ```text
   release/2.1.0
   ```

2. 完成测试

3. 合并到 main

4. 打 tag

   ```bash
   git tag v2.1.0
   git push origin v2.1.0
   ```

5. release 合并回 develop

6. 删除 release 分支

---

## 5. Nightly 版本

nightly 用于测试渠道。

格式：

```text
nightly-YYYYMMDD
```

示例：

```text
nightly-20260501
nightly-20260502
```

特点：

- 从 develop 自动构建
- 不保证稳定
- 主要用于内部测试

下游项目示例：

```yaml
wallet_sdk:
  git:
    url: xxx
    ref: nightly-20260501
```

---

## 6. SDK 依赖规则

推荐：

```yaml
ref: v2.1.0
```

不推荐：

```yaml
ref: develop
ref: main
ref: stable
```

原因：

- branch 会变化
- tag 永远指向同一版本

---

## 7. 环境规则

环境通过配置区分。

禁止：

```text
dev 分支
test 分支
prod 分支
```

正确方式：

Rust：

```bash
cargo build -p wallet-api --features dev
cargo build -p wallet-api --features test
cargo build -p wallet-api --features prod
```

---

## 8. 分支删除规则

以下分支合并后应删除：

```text
feature/*
fix/*
codex/*
release/*
hotfix/*
experiment/*
```

长期保留分支：

```text
main
develop
```

---

## 9. 提交与合并要求

合入 develop 前必须：

- cargo fmt --check
- cargo check --workspace
- cargo test --workspace
- migration 修改需说明兼容性
- FFI 修改需说明影响范围

合入 main 前必须：

- release 测试通过
- changelog 更新（如仓库维护 changelog）
- 版本号更新
- CI 全部通过

---

## 10. 推荐仓库结构

```text
repo
 ├─ src
 ├─ Cargo.toml
 ├─ README.md
 └─ docs
    └─ release
       ├─ BRANCHING.md
       ├─ RELEASE.md
       └─ CI_RELEASE.md
```

---

## 11. 最终原则

项目长期分支：

```text
main
develop
```

发布版本：

```text
v2.1.0
v2.1.1
nightly-20260501
```

开发分支：

```text
feature/*
fix/*
hotfix/*
release/*
codex/*
experiment/*
```

最终原则：

> 可变的是分支，不可变的是版本。SDK 对外交付必须依赖不可变版本。
