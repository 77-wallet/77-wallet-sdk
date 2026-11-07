# Chain configuration precedence

Scope: `wallet-api` default chain selection for Core and API Wallet.

| Invariant | Offline test |
| --- | --- |
| A chain returned disabled by the backend stays disabled in dev | `dev_does_not_restore_backend_disabled_local_chain` |
| A locally active chain absent from the backend response remains available for early dev testing | `dev_restores_local_chain_missing_from_backend` |
| Test and prod use only the backend-enabled chain selection | `test_and_prod_keep_backend_chain_selection` |
| Five EVM chain descriptors/testnet defaults stay shared; dev also supplies shared mainnet nodes/native coins. Fantom has no local seed | `added_evm_chains_are_testnet_only`, `added_evm_chains_have_native_testnet_coins`, `added_evm_chains_have_active_testnet_nodes` |
| An enabled chain missing from any wallet/account mapping requests chain synchronization | `enabled_chain_is_missing_when_any_wallet_account_lacks_it` |
| An enabled chain already present for every wallet/account mapping does not request redundant synchronization | `enabled_chain_is_complete_when_every_wallet_account_has_it` |

Test layer: unit. The tests use static local configuration and synthetic backend chain-code lists;
they perform no backend calls, database writes, or network requests.

Failure invariant: local dev fallback may supplement a missing backend chain, but must never
override an explicit backend disable. Core and API Wallet pass both the complete returned chain
set and the enabled subset through the same selection rule.
Fantom remains a recognized chain code for compatibility when supplied by the backend, but is
not locally auto-enabled as part of 2.2.

Known coverage gap: repository-level status persistence is covered by existing chain repository
tests rather than repeated here.

Run:

- `cargo test -p wallet-api --lib --no-default-features default_data::chain::tests`
- `cargo test -p wallet-api --lib --no-default-features domain::chain::tests`

## B: approved shared dev defaults (2026-10-05 simplification)

Current policy: Polygon/POL, Avalanche/AVAX, Arbitrum/ETH, Base/ETH, Scroll/ETH.
Ordinary/API wallets share static metadata and Core node storage; chain/coin tables remain separate.
Profiles are compile-time only (prod wins over dev). Cross-profile installs retaining a DB are unsupported developer misuse, not an acceptance requirement.

Fresh dev initialization creates five shared mainnet nodes through NodeDomain's existing initializer and deterministic node IDs.
The five testnet defaults remain in the shared catalog and are initialized in test builds; existing dev testnet nodes and bindings are preserved.
Existing selector ordering stays backend first, newest updated_at, then node_id; valid manual bindings stay, valid local automatic bindings stay until a backend arrives.
New node rows use insert-only defaults so reinitialization cannot revive a disabled node or replace its RPC metadata.
The redundant global visibility refresh after initialization was removed: ordinary defaults already receive profile status when written, and insert-only five-chain rows retain saved status.
API chain defaults insert missing rows, and default status refresh excludes the five dev rows to preserve backend disable/version/missing-master decisions.
API native defaults require an enabled API chain, active bound mainnet node and matching symbol; insert-only SQL rechecks chain eligibility.
API derivation resolves the API chain's node. Existing coin protocol failures remain visible.
Ordinary network coin synchronization does not disable a key present in the selected network catalog (shared native keys regression).

Removed: API-only node prefix/creation, ordinary-wallet dev exclusion, cross-profile node filtering/getter guards/CAS unbinding and their helper methods/tests.
No general replacement framework, runtime environment switch, Core/plugin changes, or transaction changes.

| Invariant | Offline evidence |
| --- | --- |
| Ordinary/API five-chain mainnet defaults share node IDs and both have native coins | `dev_both_wallets_share_five_mainnet_defaults_without_duplicate_nodes` |
| API derivation works without ordinary chain rows; completed registration is not repeated | `api_only_chain_derives_without_a_standard_chain_and_retries_unregistered_address` |
| Backend disable, version gate and missing master remain authoritative | `backend_disable_version_gate_and_missing_master_are_not_restored_by_dev_defaults`, `missing_master_backend_response_before_initialization_stays_disabled`, fake backend integration |
| Manual testnet binding and disabled API coin metadata remain intact | `dev_defaults_preserve_manual_testnet_binding_and_disabled_backend_coin` |
| Same dev DB reopen/reinitialize adds no duplicate nodes and preserves disabled node/RPC, chain, coin and ordinary/API manual binding | `same_dev_upgrade_preserves_disabled_nodes_chains_coins_and_manual_bindings` |
| Fresh test/prod/dev+prod do not inject five dev mainnet nodes or API native coins | `non_dev_builds_do_not_inject_api_mainnet_nodes_or_native_coins`, `compiled_feature_priority_and_fresh_default_injection_match_policy` |
| Ordinary default coin synchronization supports the five mainnet defaults in dev | `ordinary_coin_sync_uses_five_mainnet_defaults_in_dev` |
| Fallback coin writes recheck current chain enable/master/binding and do not replace existing rows | API-chain/API-coin/Node SQLite repository tests |
| Protocol failure writes no fallback; valid empty response can fill native defaults | `coin_protocol_failure_is_visible_and_valid_empty_response_can_fill_dev_native_coins` |

Run targeted lib config/ensurer tests with `--no-default-features --features dev`, `test`, `prod`, and `dev,prod`; database repository tests; `--test mod --features dev,integration-tests` chain_sync tests.
Current command/status evidence is in root target/api-config-simplified-results.json; logs use the api-config-simplified prefix.
The former api-config-only candidate (27 files, da098e... hash) and results are historical; they do not validate this simplified candidate.

## A: retained account expansion and recovery

A is retained, including bounded account query/registration, missing-only assets, epoch handling, Context lock, unlock hook and 30-second recovery worker.
Independent A files are SHA-256 identical to the pre-simplification snapshot. Mixed chain/backend files retain A hooks and account-sync methods.
Recorded P2 remains: a persistent earlier-chain registration failure aborts the pass and may starve later chains. B does not repair this.

#### 锁、性能与生命周期

- `api_chain_sync_lock`是每Context的Tokio互斥锁，覆盖高水位查询、所有链/分页派生、资产写入和网络注册。
  不持有数据库事务穿越HTTP，也不是进程间锁；等待不会忙轮询，但长同步期间其他同步要排队。
  本轮不改为页级锁，否则必须额外处理两次查询相同待办的竞态。节点ensure发生在该锁之外。
- 原永久AtomicBool已经被`Mutex<Option<JoinHandle<()>>>`替代。短启动锁只检查/替换句柄，不等待注册。
  活任务复用；结束、panic、abort后下一次解锁/会话轮换可重启。
  锁定/会话过期时循环保留且跳过；普通同步错误记录后下个周期重试。
  没有自动panic监督/立即重启、生产任务停止入口或新的reset状态机。
- 账户读取限制200源记录/page；写入复用既有分块，注册<=40条且<=32KiB序列化明文。
  这些限制不构成绝对内存上限：字段长度、地址变体、币种元数据及页内资产集合仍影响内存。
  加密封装最大实测43,078字节，不能把明文限制误称线上的请求总大小上限。
- high-watermark+稳定id游标覆盖本次范围，后来插入的源记录由后续扫描补齐；逻辑索引避免每页反复按钱包全扫。
  一个SQLite索引增加存储/写入维护成本；我们用临时库验证迁移与查询计划，不声称生产迁移耗时已测。

#### 可读性、耦合、抽象判断

新增编排集中在account_sync模块，复用derive_subkey_fast_with_ctx与现有init executor，
没有创建第二套扩容批次/索引分配协议。SQL集中DAO，Repo仅承接pool/事务/写锁/重试；
节点选择沿用既有共用规则，未新增 API 专用资格过滤。新模块仍包含派生、资产和注册三个步骤，
依赖Context、DAO/Repo及解锁session；这是当前真实耦合，不应把模块拆分说成已消除耦合。
两个Context字段分别保护长同步和短任务启动，职责不相同。任务启动私有helper仅服务本模块与直接单测，
不是通用调度抽象。后续若拆交付，应先分离启动循环/钱包hooks，保留补齐事实、索引、epoch与并发保护；
当前仅提出选项，未自行删除或拆分代码。

#### 需求/风险到测试的对应

| 需求/风险 | 模拟场景及直接断言 | 层级/结果与边界 |
| --- | --- | --- |
| 后台禁用/版本门槛被默认值覆盖 | 禁用Polygon、版本禁用Base，再初始化仍禁用；恰好一次链请求 | fake backend integration通过；真实后台兼容性未测 |
| 非法币种响应误写默认值 | 协议错误可见且不写兜底；合法空响应可补五原生币 | integration通过；不表示代币全集验证 |
| 两类账户扩链丢编号/名称/类型或重复注册 | 201个SubAccount+201个Withdrawal ->2010地址；并发同步总55次注册，重复调用不增加请求 | integration通过；仅五条EVM链目标 |
| 大请求越限 | 2001个地址、每个名称1200字节；全部注册、84请求、最多24条/32144明文/43078加密封装字节 | integration通过；不是已确认后台协议限制 |
| 后台注册失败/旧epoch调用 | 失败保持is_init=0，重试成功=1；调用前旧epoch不请求后台 | integration通过 |
| reset后的迟到成功污染状态 | 人工持有响应，在请求期间修改epoch再放行，完成状态仍0 | controlled-timing integration通过；跨库epoch检查并非原子事务 |
| 部分分块成功后失败/重开恢复 | 101账户/类型，首块40成功随后失败；重开Context/DB只完成剩余待办 | integration通过；不覆盖后台单个响应内部逐项部分成功 |
| 取消及真实解锁恢复 | 41账户/类型，取消未响应注册，重开DB后真实initialize_wallet_unlock_session恢复410地址，总16请求 | integration通过；不是独立进程SIGKILL |
| 重复解锁/同Context重解锁/取消worker | 检查同一任务id；锁定再解锁恢复polygon待办一次；abort实际worker后解锁产生新id并补齐 | 新integration通过；未验证锁定期间完整定时周期无查询 |
| worker异常退出重启 | 原AtomicBool的panic/restart回归先失败；真实panic完成后可再启动，活任务复用，abort后可启动 | lifecycle unit2/2通过；下一次启动入口恢复，无自动监督 |
| 大量源记录/重复/后插入/类型区分 | 80000源行、40000逻辑账户、39998待办；固定高水位分页，重复排除，后来插入留待后续 | SQLite component通过；测试显式257行/page，生产200 |
| 地址变体部分注册/主动停用 | BTC式多变体部分完成仍待办，主动停用地址保留；类型维度独立 | SQLite component通过；不是BTC真实派生/交易测试 |
| 索引迁移/回滚及恢复 | 已有行迁移、explain索引查询计划、事务回滚、重开后已完成跳过 | SQLite component通过 |
| 重试覆盖资产状态 | 保留disabled、balance、multisig、时间戳；注入trigger失败回滚，再重试补缺 | SQLite component2/2通过 |

Prior A fixture measurements are historical (80,000 source rows; 402 logical accounts to 2,010 addresses). This round reruns the relevant A component/lifecycle/integration tests; no production throughput claim.
Uncovered: real backend/RPC/transactions, per-item partial-success protocol, process SIGKILL, direct timeout test, cross-DB atomic epoch/node consistency, live account status changes and baseline node-rebinding races.
Node state checks and API coin writes use different databases; no cross-database transaction is claimed. Collection/withdrawal readiness requires a separate review and validation.

Review boundary: freeze the simplified B candidate and manifest after checks, then independent review. SDK changes remain uncommitted; no Core/plugin dependency revision is advanced.

Actual base-to-new dev upgrade: runner creates a SQLite fixture with base2dd production code, then the new dev binary fills five mainnet nodes and eligible API native coins while preserving disabled chain/node/coin metadata and existing testnet bindings. `actual_base_dev_database_upgrade_fills_defaults_and_preserves_saved_state` is ignored and requires the runner-owned offline fixture. Existing five-chain node timestamps are controlled to model old rows deterministically.

Approved five-chain ordinary default guard: only five-chain records use atomic insert-missing. Existing records remain byte-for-byte equivalent (including backend disable, manual binding and metadata); absent rows are filled. Other default chains, generic upsert and backend updates retain legacy behavior. Regression drives actual CHAIN_LIST Domain entry before defaults; DAO/Repo race component checks backend disable vs default insert and duplicate concurrent inserts. Actual base upgrade red is retained and rerun on a new disposable fixture after the fix.

## Current develop integration (2026-10-06)

Integrate only committed first-stage ef84a2e27-file delta onto2cc1a2b5. Same fixed candidate must pass five-chain dev/default disable/manual-binding guards and existing withdrawal_message_center/global_business_query service+repository regressions. Original A pagination/worker/index/price changes are excluded. Fourfeature profiles(dev/test/prod/dev+prod), bounded affected SQLite filters, localfake chain_sync andfmt; preserve current message-center/dual-business implementation hashes outside B scope. This local dependency integration does not assert App/live or50万-address capacity.

### Approved A four-column index follow-up (uncommitted)

`api_account_logical_status_id_idx(wallet_address, account_id, api_wallet_type, status)` uses implicit rowid for earlier-active id range. Fresh schema and repeated DDL/query plans checked; existing five-column same-name index stays five under IF NOT EXISTS. SQLx already-applied five-column checksum rejects edited migration before change (VersionMismatch); no checksum bypass/automatic existing-five upgrade. Old installs would need a separately reviewed forward rebuild migration or explicit development-fixture replacement, neither performed here. Existing same SQLite3.46 benchmark50万 uniqueaddresses/10万logical/50万rows retained; phone/coldstorage/SQLx end-to-end not measured.

### Stage2 A persistent queue continuation (current candidate, validation pending)

Candidate b69436c6 base retains B configuration/message-center behavior. Historical direct worker/synchronous error results above are superseded for A recovery. Current A creates per-chain local `ReconcileApiChainAccounts` parents and stable-ID backend `ADDRESS_INIT` children in the existing durable queue; no separate recovery worker. Registration uses the original executor with post-response epoch protection. Only A-owned rows use persisted capped exponential backoff and the existing manager's five-second wake; no ten-attempt terminal hang. Other task policies remain unchanged.

| Boundary | Required assertion | Evidence target |
| --- | --- | --- |
| Source scan | Logical keyset page 200/high-watermark, both types, disabled/name/index/variants preserved; four-column index migration | `missing_chain_tests`, chain_sync repeat/concurrent tests |
| Durable gap | Address/default assets survive before child insertion; unfinished parent reconstructs first and future pages | `pre_enqueue_gap_and_uncreated_future_pages_resume_from_durable_parent` |
| Admission/backoff | Atomic 64 pending/running admission on insert/rearm/split/failed claim; durable waiting parents outside active window; bounded due reads, deadlines exclude early retry, failed rows yield later due work, >10 retries remain eligible | task_queue SQLite + same-manager wake tests |
| Parent/child replacement | Two children and replacement commit together or roll back; duplicate split idempotent, late failure cannot undo completion | `api_chain_split_commits_children_and_replacement_atomically_or_rolls_back` + full-capacity/rearm/claim regressions |
| Address-local failure | Only documented backend 8050001 (address not found) may bisect; 400/422/8020001/auth/rate/network do not. At most 2*n-1 localization nodes; singleton retains backoff, healthy siblings advance | A classifier unit + rejected-address integration |
| Oversized item | Keep oversized name/address init0 and observable parent failure, flush healthy entries before/after it and generate later source pages; transport bounds remain 40/32KiB | `oversized_item_stays_pending_while_healthy_chunks_and_future_pages_complete` (red 1000 vs1010 retained) |
| Lifecycle | Latest state read before consuming stale delivery; completed ID reactivates for pending facts; one dispatcher guard retains reservation through category/permit cleanup; cancellation/panic releases that reservation; checker starts once; same running manager wakes after deadline without restart/unlock | lifecycle/duplicate/reactivation and same-manager integration |
| Isolation | Failed registration does not block independent chunks/chains; DB marker errors stay failed with addresses init0 | partial/all/first-chain and DB-trigger integration |
| Epoch | Stale epoch skips backend; response arriving after reset does not mark init; queued later stale children finish without backend | executor and epoch integration |
| Unlock/reopen | Persisted jobs/addresses recover via actual unlock path and manager, repeated unlock keeps stable parent IDs | unlock/cancellation/reopen integration |

Backend contract evidence: `wallet-transport-backend/src/api/api_wallet/address.rs` returns whole-request `Result<(), Error>`; `response/api_response.rs` preserves `ApiBackend(code,msg)` only. `docs/reference/error_codes.md` defines 8050001 as address-local; no per-item ACK or generic 400/422 address contract is assumed. Unknown/non-address failures remain whole-batch retries.

No App/live/real backend/ABI or 500000-address end-to-end acceptance performed. Existing same-name five-column development migration checksum issue remains as documented above; no bypass or automatic rebuild.

### A boundary adjustment — current business coordinator (new validation required)

Supersedes the previous manager-owned wake/lifecycle description above. Missing-chain `chain/recovery.rs` is the sole consumer: Context-local reservations, idempotent supervisor startup, five-second wake, up to four concurrent attempts, persisted deadlines/split, timeout and cancellation cleanup. App startup starts it before default queue recovery. Same running coordinator retries beyond ten attempts without restart or unlock. Original registration executor, stable durable IDs and facts remain unchanged. No new table is created.

Generic routing contract: `task_queue.remark` containing valid JSON with a top-level **nonempty string `owner`** explicitly reserves that record for an external consumer. Unknown owners obey this protocol and remain externally owned until their consumer is available; the default consumer neither parses/deletes nor executes them. Ordinary remarks, absent/empty owner, malformed JSON, null, numeric, boolean, object, array and nested-only owner retain default routing/cleanup policy. Unfinished external records survive age cleanup; completed records remain subject to ordinary cleanup. Generic DAO/Repo receive explicit owner/status/capacity; they know neither the A task name nor 64. The chain repository owns due query, limit and A storage policy.

Compatibility inventory at b69436c6/current source: TaskItem new/new_with_id initialize remark=None; push_with_remark and update_task_remark have no business callers; Task builders forward this optional field; B address-balance task directly passes None. Legacy core schema has no remark; list_legacy_core fills NULL during migration. Current task.db schema defaults remark NULL. No other existing top-level task remark owner writer was found. This is source/history evidence, not an inspection of production databases or arbitrary externally hand-edited historical rows. Fee-result payload remarks and multisig notes belong to other entities, not queue routing metadata.

Evidence: `remark_routing_and_retention_are_compatible_for_all_json_owner_types` covers fourteen remark forms across statuses0–4; `default_recovery_excludes_every_external_owner_before_parsing_names` checks other owner/unknown name; `unknown_external_owner_is_retained_before_generic_name_parsing` drives the default dispatch entry; same-coordinator integration also injects each owned failure row into the live generic dispatcher and requires one HTTP call per attempt. Existing gap, future-page, unlock, bad-chain/address, capacity, epoch and reactivation assertions remain mandatory. New results are recorded under root target/api-A-boundary-*; old api-A-reviewed-final-* remains a separate checkpoint.


### P2 routing closure — SQLite is the single metadata authority

The earlier Rust `Value` classifier is removed. Generic recovery selection, retention and all dispatch/execution entry gates use the same SQLite predicate (`json_valid` + `json_type` + `json_extract`). None metadata keeps default routing without a query. Duplicate decoded owner keys use SQLite's first matching key; this includes escaped equivalent keys. Deep unrelated fields and unused numbers such as1e999 cannot silently fall back to default because of Rust parser limits. No business owner string, task kind or capacity is introduced into generic routing.

`remark_routing_and_retention_are_compatible_for_all_json_owner_types` now covers21 forms × states0–4, including both duplicate directions, escaped keys,140 layers and1e999. The actual default-dispatch fixture deliberately does not pre-classify metadata;20 external unknown-name rows must survive direct dispatch, startup recovery selection and aged cleanup unchanged. Same-coordinator12-failure regression additionally routes owned registration records into the live default dispatcher and asserts exactly one backend attempt per iteration. Historical red exit101 is retained in `api-A-boundary-p2-red.log`; fresh final results/manifests and independent follow-up review are separate root-target artifacts.

- 补链观测：chain::recovery 单元断言日志 ID 为摘要，错误仅固定分类/数字码，不回显动态敏感文本；chain_sync 验证新增日志不改变持久恢复语义。分页/退避/拆批/完成日志在对应持久操作成功后记录，扫描完成不代表后台注册完成。


## Ordinary default assets after catalog sync (2026-10-08)

Approved scope: successful ordinary `CoinService::pull_hot_coins` reconciles missing default assets for existing active ordinary accounts. No key derivation, address registration, API/multisig account query, backend default changes or real-wallet operations. Eligibility requires active wallet/account/chain and `coin.is_default=1, is_del=0, status=1`. Existing ordinary initialization query is unchanged.

Any existing address/chain/token asset (including status0 manual removals, multisig rows, old symbol and NULL native representations) blocks insertion. Insert-only writes preserve all old asset values. Duplicate catalog symbols for one token produce one asset. The existing coin precision-repair owner remains unchanged; this reconciliation does not perform precision or balance updates.

| Invariant | Regression |
| --- | --- |
| New default token is added with zero balance to an existing account; repeat leaves saved row unchanged | `ordinary_default_assets_insert_existing_account_token_and_repeat_is_noop` |
| Non-default/deleted/disabled coins, disabled wallet/account/chain and nonmatching chains excluded | `ordinary_default_assets_filter_coin_and_account_scope`, `ordinary_default_assets_disabled_chain_and_multisig_only_address_are_excluded` |
| Soft removals, saved balances/status/timestamps and existing multisig rows never overwritten/reactivated | `ordinary_default_assets_preserve_hidden_balances_and_multisig_even_with_old_symbol`, `ordinary_default_assets_multiple_pages_and_concurrent_runs_are_idempotent` |
| 200-candidate/400-bind pages complete451 accounts; concurrent runs do not duplicate; failed write page retry completes without losing prior pages | `ordinary_default_assets_multiple_pages_and_concurrent_runs_are_idempotent`, `ordinary_default_assets_failed_page_preserves_prior_pages_and_retry_completes` |
| A hidden row or disabled account introduced after selection is honored at insert; catalog token aliases deduplicated | `ordinary_default_assets_recheck_hidden_and_disabled_between_selection_and_insert`, `ordinary_default_assets_same_token_aliases_create_one_row` |
| No assets before last backend page; successful completion fills, emits one refresh, repeat emits no additional refresh; API table unchanged | `ordinary_default_assets_wait_for_last_page_then_fill_and_repeat_safely` |
| Failed last page, failed catalog completion or cancellation do not fill/notify early; next successful sync completes | `ordinary_default_assets_failed_later_page_writes_no_partial_assets_or_coins`, `ordinary_default_assets_failed_catalog_completion_defers_fill_until_retry`, `ordinary_default_assets_cancelled_last_page_does_not_write_or_notify` |

Account/coin rowid upper bounds and keyset cursor keep candidate memory bounded; each write statement inserts at most200 rows. Completed pages survive interruption and a retry re-evaluates current DB facts. Rows added beyond the captured upper bounds or made eligible behind the cursor are deferred to a later successful sync. Existing metadata out-of-order semantics are unchanged; no stale result overwrites asset state. Frontend refresh delivery is best effort after durable rows, not a durable notification protocol.

Validation is offline SQLite and local HTTP fakes.451 synthetic ordinary accounts is bounded regression evidence, not500k scale acceptance. No App/device/end-to-end or real backend/chain acceptance claimed.
