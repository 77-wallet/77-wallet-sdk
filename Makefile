# Rust 项目 Makefile
# 使用: make [target]
# 例如: make fmt, make clippy, make check, make release

# 变量
CARGO := cargo

# 默认目标
.DEFAULT_GOAL := help

# ---------- 基础命令 ----------

fmt: ## 格式化所有代码
	@$(CARGO) fmt --all

fmt-check: ## 检查格式 (不修改)
	@$(CARGO) fmt --all -- --check

clippy: ## 运行 Clippy 检查 (包括 tests/examples)
	@$(CARGO) clippy --all-targets --all-features -- -D warnings

test: ## 运行所有单元测试
	@$(CARGO) test --all

build: ## 构建 debug 版本
	@$(CARGO) build

release: ## 构建 release 版本
	@$(CARGO) build --release

clean: ## 清理构建文件
	@$(CARGO) clean

run: ## 运行主程序
	@$(CARGO) run

check: fmt-check clippy test ## 综合检查

# ---------- 辅助功能 ----------

help: ## 显示帮助信息
	@echo "📦 Rust Makefile 常用命令:"
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) | \
	awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'
