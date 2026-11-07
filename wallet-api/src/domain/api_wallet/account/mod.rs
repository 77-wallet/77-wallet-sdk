pub(crate) mod search;
use crate::{
    context::Context,
    domain::{
        account::AccountDomain,
        api_wallet::{assets::ApiAssetsDomain, chain::ApiChainDomain, wallet::ApiWalletDomain},
        app::config::ConfigDomain,
        chain::ChainDomain,
        wallet::WalletDomain,
    },
    error::service::ServiceError,
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        common::CommonTask,
        task::Tasks,
    },
    messaging::notify::{
        FrontendNotifyEvent, api_wallet::AwmCmdAddrExpandMsgFront, event::NotifyEvent,
    },
    response_vo::{
        api_wallet::account::{ApiAccountChainInfo, ApiAccountInfo},
        standard_wallet::account::BalanceInfo,
    },
    service::api_wallet::asset::AddressChainCode,
};
use once_cell::sync::OnceCell;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_crypto::EncryptedJsonGenerator as _;
use wallet_database::{
    entities::{
        address_query_state::{AddressQueryStateEntity, AddressQueryStatus},
        api_account::CreateApiAccountVo,
        api_wallet::ApiWalletType,
    },
    pagination::Pagination,
    repositories::{
        api_wallet::{
            account::ApiAccountRepo, address_query_state::AddressQueryStateRepo,
            asset_query_state::AssetQueryStateRepo, assets::ApiAssetsRepo, coin::ApiCoinRepo,
            expand_batch_item::ExpandBatchItemRepo, expand_notify_state::ExpandNotifyStateRepo,
            wallet::ApiWalletRepo,
        },
        exchange_rate::ExchangeRateRepo,
    },
};
use wallet_transport_backend::request::{
    AddressInitReq, TokenQueryPriceReq, api_wallet::address::ApiAddressInitReq,
};
use wallet_types::chain::{address::r#type::AddressType, chain::ChainCode};

pub(crate) struct ApiAccountDomain {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExistingAccountCheck {
    CheckEach,
    PrefilteredMissing,
}

impl ExistingAccountCheck {
    fn is_required(self) -> bool {
        matches!(self, Self::CheckEach)
    }
}

/// 延迟执行数据结构
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct CreateAccountDeferredData {
    api_wallet_uid: String,
    api_wallet_address: String,
    created_addresses: Vec<String>,
    chain_code: String,
    api_address_init_req: ApiAddressInitReq,
    is_recover: bool,
    is_last_page: bool, // ⭐ 添加：是否最后一页
}

impl CreateAccountDeferredData {
    pub(crate) fn recovery_uid(&self) -> Option<&str> {
        self.is_recover.then_some(self.api_wallet_uid.as_str())
    }
}

#[derive(Debug)]
struct ExpandProgressMsg {
    uid: String,
    chain_code: String,
    wallet_address: String,
    indices: Vec<i32>,
    page_size: i32,
}

#[derive(Debug)]
struct AddressRecoveryProgressMsg {
    uid: String,
    chain_code: String,
    page_num: i32,
    done_number: u32,
    total_number: u32,
    is_last_page: bool,
}

enum PageNotifyMsg {
    ExpandProgress(ExpandProgressMsg),
    AddressRecoveryProgress(AddressRecoveryProgressMsg),
}

#[derive(Debug, Clone, Copy)]
struct AddressRecoveryPageState {
    done_number: u32,
    total_number: u32,
    is_last_page: bool,
}

#[derive(Debug)]
struct AddressRecoveryChainState {
    loaded_last_notified: bool,
    last_notified_page: i32,
    pending_pages: BTreeMap<i32, AddressRecoveryPageState>,
    pending_done_number: u32,
    pending_total_number: u32,
    pending_completion: bool,
}

#[derive(Debug)]
struct PageNotifyState {
    wallet_address: String,
    created_indices_by_chain: HashMap<String, HashSet<i32>>,
    last_notified_page: i32,
    loaded_last_notified: bool,
    page_size: i32,
    dirty: bool,
    last_seen_max_page: i32,
    pending_expand_notify_count: u32,
    address_recovery_by_chain: HashMap<String, AddressRecoveryChainState>,
    last_emit_at: Option<Instant>,
}

const SUB_ACCOUNT_PAGE_SIZE: i32 = 10;
const WITHDRAWAL_PAGE_SIZE: i32 = 2;
const NOTIFY_STATE_CHAIN_KEY: &str = "*";
const ADDRESS_RECOVERY_NOTIFY_STATE_PREFIX: &str = "addr_recovery:";
const PAGE_NOTIFY_TICK: Duration = Duration::from_secs(1);
const EXPAND_NOTIFY_BATCH_WINDOW: Duration = Duration::from_secs(5);

static PAGE_NOTIFY_TX: OnceCell<mpsc::Sender<PageNotifyMsg>> = OnceCell::new();

fn page_notify_tx(ctx: &'static Context) -> &'static mpsc::Sender<PageNotifyMsg> {
    PAGE_NOTIFY_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel(1000);
        tokio::spawn(async move {
            run_page_notify_loop(ctx, rx).await;
        });
        tx
    })
}

async fn run_page_notify_loop(ctx: &'static Context, mut rx: mpsc::Receiver<PageNotifyMsg>) {
    let mut states: HashMap<String, PageNotifyState> = HashMap::new();
    let mut interval = tokio::time::interval(PAGE_NOTIFY_TICK);

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                match msg {
                    PageNotifyMsg::ExpandProgress(msg) => {
                        let key = msg.uid.clone();
                        let state = states.entry(key).or_insert_with(|| {
                            tracing::info!(
                                uid = %msg.uid,
                                "Expand notify: creating new state (last_notified_page=-1)"
                            );
                            PageNotifyState {
                                wallet_address: msg.wallet_address.clone(),
                                created_indices_by_chain: HashMap::new(),
                                last_notified_page: -1,
                                loaded_last_notified: false,
                                page_size: msg.page_size,
                                dirty: true,
                                last_seen_max_page: -1,
                                pending_expand_notify_count: 0,
                                address_recovery_by_chain: HashMap::new(),
                                last_emit_at: None,
                            }
                        });
                        tracing::info!(
                            uid = %msg.uid,
                            chain_code = %msg.chain_code,
                            indices_count = msg.indices.len(),
                            page_size = msg.page_size,
                            last_notified_page = state.last_notified_page,
                            "Expand notify: received indices batch"
                        );
                        if state.wallet_address.is_empty() {
                            state.wallet_address = msg.wallet_address.clone();
                        }
                        if state.page_size == 0 {
                            state.page_size = msg.page_size;
                        }
                        let entry = state
                            .created_indices_by_chain
                            .entry(msg.chain_code.clone())
                            .or_insert_with(HashSet::new);
                        let before_len = entry.len();
                        entry.extend(msg.indices.into_iter());
                        let after_len = entry.len();
                        if after_len != before_len {
                            state.dirty = true;
                        }
                        tracing::info!(
                            uid = %msg.uid,
                            chain_code = %msg.chain_code,
                            created_indices_count = state
                                .created_indices_by_chain
                                .get(&msg.chain_code)
                                .map(|s| s.len())
                                .unwrap_or(0),
                            last_notified_page = state.last_notified_page,
                            "Expand notify: merged indices into state"
                        );
                        if !state.dirty {
                            tracing::info!(
                                uid = %msg.uid,
                                chain_code = %msg.chain_code,
                                "Expand notify: no new indices merged, skipping re-eval"
                            );
                        }
                    }
                    PageNotifyMsg::AddressRecoveryProgress(msg) => {
                        let key = msg.uid.clone();
                        let state = states.entry(key).or_insert_with(|| PageNotifyState {
                            wallet_address: String::new(),
                            created_indices_by_chain: HashMap::new(),
                            last_notified_page: -1,
                            loaded_last_notified: false,
                            page_size: SUB_ACCOUNT_PAGE_SIZE,
                            dirty: false,
                            last_seen_max_page: -1,
                            pending_expand_notify_count: 0,
                            address_recovery_by_chain: HashMap::new(),
                            last_emit_at: None,
                        });

                        let chain_state = state
                            .address_recovery_by_chain
                            .entry(msg.chain_code.clone())
                            .or_insert_with(|| AddressRecoveryChainState {
                                loaded_last_notified: false,
                                last_notified_page: -1,
                                pending_pages: BTreeMap::new(),
                                pending_done_number: 0,
                                pending_total_number: 0,
                                pending_completion: false,
                            });

                        chain_state
                            .pending_pages
                            .entry(msg.page_num)
                            .and_modify(|existing| {
                                existing.done_number =
                                    existing.done_number.max(msg.done_number);
                                existing.total_number =
                                    existing.total_number.max(msg.total_number);
                                existing.is_last_page = existing.is_last_page || msg.is_last_page;
                            })
                            .or_insert(AddressRecoveryPageState {
                                done_number: msg.done_number,
                                total_number: msg.total_number,
                                is_last_page: msg.is_last_page,
                            });

                        tracing::info!(
                            uid = %msg.uid,
                            chain_code = %msg.chain_code,
                            page_num = msg.page_num,
                            done_number = msg.done_number,
                            total_number = msg.total_number,
                            is_last_page = msg.is_last_page,
                            "Address recovery notify: enqueued page progress"
                        );
                    }
                }
            }
            _ = interval.tick() => {
                if let Err(e) = try_notify_pages(ctx, &mut states).await {
                    tracing::warn!(error = %e, "page notify loop: try_notify_pages failed");
                }
            }
            else => break,
        }
    }
}

async fn flush_batched_expand_notify(
    uid: &str,
    state: &mut PageNotifyState,
    now: Instant,
    ctx: &'static Context,
) -> Result<(), ServiceError> {
    if state.pending_expand_notify_count == 0 {
        return Ok(());
    }

    let should_send = match state.last_emit_at {
        Some(last) => last.elapsed() >= EXPAND_NOTIFY_BATCH_WINDOW,
        None => true,
    };

    if !should_send {
        return Ok(());
    }

    let count = state.pending_expand_notify_count;
    let notify_data =
        AwmCmdAddrExpandMsgFront { uid: uid.to_string(), number: count, done_number: count };

    FrontendNotifyEvent::new(NotifyEvent::AwmCmdAddrExpand(notify_data)).send_with_ctx(ctx).await?;
    state.pending_expand_notify_count = 0;
    state.last_emit_at = Some(now);

    tracing::info!(
        uid = %uid,
        batched_count = count,
        "Expand notify: sent batched AWM_CMD_ADDR_EXPAND"
    );

    Ok(())
}

fn build_address_recovery_notify_state_key(chain_code: &str) -> String {
    format!("{ADDRESS_RECOVERY_NOTIFY_STATE_PREFIX}{chain_code}")
}

fn fold_pending_address_recovery_pages(chain_state: &mut AddressRecoveryChainState) -> bool {
    let mut advanced = false;
    let mut consumed_pages: Vec<i32> = Vec::new();
    for (page, progress) in chain_state.pending_pages.iter() {
        if *page <= chain_state.last_notified_page {
            consumed_pages.push(*page);
            continue;
        }
        chain_state.pending_done_number =
            chain_state.pending_done_number.saturating_add(progress.done_number);
        if progress.total_number > 0 {
            chain_state.pending_total_number = progress.total_number;
        }
        chain_state.pending_completion = chain_state.pending_completion || progress.is_last_page;
        chain_state.last_notified_page = *page;
        consumed_pages.push(*page);
        advanced = true;
    }
    for page in consumed_pages {
        chain_state.pending_pages.remove(&page);
    }
    advanced
}

async fn reconcile_address_recovery_pages(
    pool: &wallet_database::ApiWalletDbPool,
    uid: &str,
    state: &mut PageNotifyState,
) -> Result<(), ServiceError> {
    for (chain_code, chain_state) in state.address_recovery_by_chain.iter_mut() {
        if !chain_state.loaded_last_notified {
            let key = build_address_recovery_notify_state_key(chain_code);
            let last = ExpandNotifyStateRepo::get_by_uid_and_chain(pool, uid, &key)
                .await?
                .map(|e| e.last_notified_page as i32)
                .unwrap_or(-1);
            chain_state.last_notified_page = last;
            chain_state.loaded_last_notified = true;
        }

        let original_last_page = chain_state.last_notified_page;
        let advanced = fold_pending_address_recovery_pages(chain_state);
        if advanced && chain_state.last_notified_page != original_last_page {
            let key = build_address_recovery_notify_state_key(chain_code);
            if let Err(e) = ExpandNotifyStateRepo::update_last_notified_page(
                pool,
                uid,
                &key,
                chain_state.last_notified_page as i64,
            )
            .await
            {
                tracing::warn!(
                    uid = %uid,
                    chain_code = %chain_code,
                    last_notified_page = chain_state.last_notified_page,
                    error = %e,
                    "Address recovery notify: failed to persist last_notified_page"
                );
            }
        }
    }
    Ok(())
}

fn take_pending_address_recovery_notify(
    uid: &str,
    state: &mut PageNotifyState,
) -> Option<AwmCmdAddrExpandMsgFront> {
    let mut chain_codes: Vec<String> = state.address_recovery_by_chain.keys().cloned().collect();
    chain_codes.sort_unstable();
    for chain_code in chain_codes {
        let Some(chain_state) = state.address_recovery_by_chain.get_mut(&chain_code) else {
            continue;
        };
        let should_send = chain_state.pending_done_number > 0 || chain_state.pending_completion;
        if !should_send {
            continue;
        }
        let payload = AwmCmdAddrExpandMsgFront {
            uid: uid.to_string(),
            done_number: chain_state.pending_done_number,
            number: chain_state.pending_total_number,
        };
        chain_state.pending_done_number = 0;
        chain_state.pending_completion = false;
        return Some(payload);
    }
    None
}

fn can_emit(last_emit_at: Option<Instant>, now: Instant) -> bool {
    match last_emit_at {
        Some(last) => now.saturating_duration_since(last) >= EXPAND_NOTIFY_BATCH_WINDOW,
        None => true,
    }
}

async fn try_notify_pages(
    ctx: &'static Context,
    states: &mut HashMap<String, PageNotifyState>,
) -> Result<(), ServiceError> {
    let pool = ctx.api_wallet_pool()?;
    let now = Instant::now();

    for (uid, state) in states.iter_mut() {
        if !state.dirty {
            reconcile_address_recovery_pages(&pool, uid, state).await?;
            if can_emit(state.last_emit_at, now) {
                if let Some(notify_data) = take_pending_address_recovery_notify(uid, state) {
                    FrontendNotifyEvent::new(NotifyEvent::AddressRecovery(notify_data))
                        .send_with_ctx(ctx)
                        .await?;
                    state.last_emit_at = Some(now);
                    continue;
                }
            }
            flush_batched_expand_notify(uid, state, now, ctx).await?;
            continue;
        }
        let inited_by_chain: HashMap<String, HashSet<i32>> = {
            let mut map = HashMap::new();
            for chain_code in state.created_indices_by_chain.keys() {
                let inited =
                    ApiAccountRepo::list_inited_indices(&pool, &state.wallet_address, chain_code)
                        .await?
                        .into_iter()
                        .map(|(idx,)| idx)
                        .collect::<HashSet<i32>>();
                map.insert(chain_code.clone(), inited);
            }
            map
        };

        if !state.loaded_last_notified {
            let last =
                ExpandNotifyStateRepo::get_by_uid_and_chain(&pool, uid, NOTIFY_STATE_CHAIN_KEY)
                    .await?
                    .map(|e| e.last_notified_page as i32)
                    .unwrap_or(-1);
            state.last_notified_page = last;
            state.loaded_last_notified = true;
            tracing::info!(
                uid = %uid,
                last_notified_page = state.last_notified_page,
                "Expand notify: loaded last_notified_page from DB"
            );
        }

        tracing::info!(
            uid = %uid,
            chains = ?state.created_indices_by_chain.keys().collect::<Vec<_>>(),
            last_notified_page = state.last_notified_page,
            "Expand notify: evaluating pages"
        );
        let mut keep_dirty = false;
        let page_size = if state.page_size > 0 { state.page_size } else { SUB_ACCOUNT_PAGE_SIZE };
        let mut advanced_pages = 0i32;
        let original_last_notified_page = state.last_notified_page;
        loop {
            let next_page = state.last_notified_page + 1;
            let page_start = next_page * page_size;
            let page_end = page_start + page_size - 1;

            let max_page = state
                .created_indices_by_chain
                .values()
                .filter_map(|set| set.iter().max().copied())
                .map(|max_idx| max_idx / page_size)
                .min()
                .unwrap_or(-1);

            if next_page > max_page {
                if max_page != state.last_seen_max_page {
                    tracing::info!(
                        uid = %uid,
                        next_page = next_page,
                        max_page = max_page,
                        "Expand notify: next page exceeds max created page, stop checking"
                    );
                    state.last_seen_max_page = max_page;
                }
                break;
            }

            let mut page_created = true;
            let mut page_inited = true;
            let mut missing_inited: Vec<(String, Vec<i32>)> = Vec::new();
            for (chain_code, created_set) in state.created_indices_by_chain.iter() {
                let mut missing_for_chain: Vec<i32> = Vec::new();
                for idx in page_start..=page_end {
                    if !created_set.contains(&idx) {
                        page_created = false;
                        break;
                    }
                    if let Some(inited_set) = inited_by_chain.get(chain_code) {
                        if !inited_set.contains(&idx) {
                            page_inited = false;
                            missing_for_chain.push(idx);
                        }
                    }
                }
                if !missing_for_chain.is_empty() {
                    missing_inited.push((chain_code.clone(), missing_for_chain));
                }
                if !page_created || !page_inited {
                    break;
                }
            }

            tracing::info!(
                uid = %uid,
                page = next_page,
                page_start = page_start,
                page_end = page_end,
                page_size = page_size,
                max_page = max_page,
                page_created = page_created,
                page_inited = page_inited,
                "Expand notify: page readiness check"
            );

            if !page_created || !page_inited {
                if page_created && !page_inited {
                    keep_dirty = true; // 等待初始化完成，继续评估
                    tracing::info!(
                        uid = %uid,
                        page = next_page,
                        missing_inited = ?missing_inited,
                        "Expand notify: waiting for init completion on page"
                    );
                }
                break;
            }

            tracing::info!(
                uid = %uid,
                page = next_page,
                page_start = page_start,
                page_end = page_end,
                "Expand notify: page initialized and accepted for batched notify"
            );

            state.last_notified_page = next_page;
            advanced_pages += 1;
        }

        if state.last_notified_page != original_last_notified_page {
            if let Err(e) = ExpandNotifyStateRepo::update_last_notified_page(
                &pool,
                uid,
                NOTIFY_STATE_CHAIN_KEY,
                state.last_notified_page as i64,
            )
            .await
            {
                tracing::warn!(
                    uid = %uid,
                    last_notified_page = state.last_notified_page,
                    error = %e,
                    "Expand notify: failed to persist last_notified_page"
                );
            }
        }

        if advanced_pages > 0 {
            let advanced_count = (advanced_pages * page_size) as u32;
            state.pending_expand_notify_count =
                state.pending_expand_notify_count.saturating_add(advanced_count);
            tracing::info!(
                uid = %uid,
                advanced_pages = advanced_pages,
                advanced_count = advanced_count,
                pending_notify_count = state.pending_expand_notify_count,
                "Expand notify: batched progress updated"
            );
        }

        state.dirty = keep_dirty;
        reconcile_address_recovery_pages(&pool, uid, state).await?;
        if can_emit(state.last_emit_at, now) {
            if let Some(notify_data) = take_pending_address_recovery_notify(uid, state) {
                FrontendNotifyEvent::new(NotifyEvent::AddressRecovery(notify_data))
                    .send_with_ctx(ctx)
                    .await?;
                state.last_emit_at = Some(now);
            } else {
                flush_batched_expand_notify(uid, state, now, ctx).await?;
            }
        }
    }

    Ok(())
}

impl ApiAccountDomain {
    fn resolve_recover_start_page(
        current_state: Option<&AddressQueryStateEntity>,
        fallback_last_page: i64,
    ) -> i32 {
        let last_page = current_state.map(|state| state.last_page).unwrap_or(fallback_last_page);

        if last_page < 0 { 0 } else { last_page as i32 + 1 }
    }

    /// Queue one address-recovery page progress event for unified throttled notification.
    ///
    /// This is best-effort user-facing progress reporting. Core recovery flow must keep
    /// functioning even when the notify channel is unavailable.
    pub(crate) async fn enqueue_address_recovery_progress_with_ctx(
        ctx: &'static Context,
        uid: &str,
        chain_code: &str,
        page_num: i32,
        done_number: u32,
        total_number: u32,
        is_last_page: bool,
    ) -> Result<(), ServiceError> {
        let msg = PageNotifyMsg::AddressRecoveryProgress(AddressRecoveryProgressMsg {
            uid: uid.to_string(),
            chain_code: chain_code.to_string(),
            page_num,
            done_number,
            total_number,
            is_last_page,
        });
        page_notify_tx(ctx).send(msg).await.map_err(|e| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "address recovery notify queue send failed: {e}"
                )),
            )
        })
    }

    // Historical list_api_accounts implementation removed during context-boundary cleanup.

    pub(crate) async fn list_api_accounts_v2_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiAccountInfo>, ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let core_pool = ctx.core_pool()?;
        let chain_code_for_log = chain_code.clone();
        tracing::info!(
            wallet_address = %wallet_address,
            account_id = ?account_id,
            chain_code = ?chain_code_for_log,
            page,
            page_size,
            "api_wallet.account.list_api_accounts_v2 domain start"
        );

        let account_ids_en = ApiAccountRepo::lists_acc_by_wallet_address_v3(
            &pool,
            wallet_address,
            account_id,
            chain_code.clone(),
            page,
            page_size,
        )
        .await?;
        tracing::info!(
            wallet_address = %wallet_address,
            account_id = ?account_id,
            chain_code = ?chain_code_for_log,
            page,
            page_size,
            page_account_count = account_ids_en.len(),
            "api_wallet.account.list_api_accounts_v2 fetched page ids"
        );

        let account_ids: Vec<_> = account_ids_en.iter().map(|acc| acc.account_id).collect();

        let account_assert = ApiAccountRepo::lists_by_wallet_address_v3(
            &pool,
            wallet_address,
            account_ids,
            chain_code.clone(),
        )
        .await?;
        let account_assert_total = ApiAccountRepo::count_by_wallet_address_v3(
            &pool,
            wallet_address,
            account_id,
            chain_code.clone(),
        )
        .await?;
        tracing::info!(
            wallet_address = %wallet_address,
            account_id = ?account_id,
            chain_code = ?chain_code,
            page,
            page_size,
            page_account_count = account_ids_en.len(),
            detail_account_count = account_assert.len(),
            total_count = account_assert_total,
            "api_wallet.account.list_api_accounts_v2 domain finished"
        );

        let currency = ConfigDomain::get_currency(ctx).await?;
        let exchange_rate =
            ExchangeRateRepo::get_by_target_currency_or_default(core_pool, &currency).await?;
        let cal_exchange_rate = |value: f64| {
            if exchange_rate.target_currency.to_uppercase() == "USD" {
                value
            } else {
                value * exchange_rate.rate
            }
        };

        let mut result: Vec<_> = vec![];
        for acc in account_assert {
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(acc.account_id)?;

            let mut chain_vec = vec![];
            let mut has_chain = HashSet::new();
            for one in acc.get_chain_info_list()?.into_iter() {
                let address_type =
                    AccountDomain::get_show_address_type(&one.chain_code, one.address_type())?;
                let r = ApiAccountChainInfo {
                    address: one.account_address,
                    derivation_path: one.derivation_path,
                    chain_code: one.chain_code,
                    name: one.chain_name,
                    address_type,
                };
                if has_chain.contains(&r.chain_code) {
                    continue;
                }
                has_chain.insert(r.chain_code.clone());
                chain_vec.push(r);
                //
                // break;
            }

            result.push(ApiAccountInfo {
                chain: chain_vec,
                account_id: acc.account_id,
                account_index_map,
                name: acc.account_name,
                balance: BalanceInfo {
                    amount: acc.total_coins_quantity,
                    currency: currency.clone(),
                    unit_price: acc.coin_unit_price.map(cal_exchange_rate),
                    fiat_value: acc.total_account_amount.map(cal_exchange_rate),
                },
                api_wallet_type: acc.api_wallet_type,
            })
        }

        Ok(Pagination { page, page_size, total_count: account_assert_total, data: result })
    }

    /// 从种子生成私钥的公共函数
    pub(crate) async fn generate_private_key_from_seed_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        chain_code: &str,
        address_type: &AddressType,
        account_id: u32,
    ) -> Result<Vec<u8>, crate::error::service::ServiceError> {
        // 解密种子
        let seed = ApiWalletDomain::new(ctx).get_seed(wallet_address).await?;

        // 转换链码
        let code: ChainCode = chain_code.try_into()?;

        // 获取节点信息
        let node = ChainDomain::get_node_with_ctx(ctx, chain_code).await?;

        // 创建链实例
        let instance: wallet_chain_instance::instance::ChainObject = (
            &code,
            address_type,
            crate::domain::chain::ChainDomain::network_kind_from_node_network(&node.network),
        )
            .try_into()?;

        // 解析账户索引
        let account_index_map =
            wallet_utils::address::AccountIndexMap::from_account_id(account_id)?;

        // 生成密钥对
        let keypair =
            instance.gen_keypair_with_index_address_type(&seed, account_index_map.input_index)?;

        // 获取私钥字节
        let res = keypair.private_key_bytes()?;
        Ok(res)
    }

    /// 加密私钥的公共函数
    pub(crate) async fn encrypt_private_key(
        ctx: &'static Context,
        password: &str,
        private_key_bytes: &[u8],
    ) -> Result<String, crate::error::service::ServiceError> {
        use crate::domain::app::config::ConfigDomain;
        use rand::rngs::OsRng;
        use wallet_crypto::KeystoreJsonGenerator;

        // 获取加密算法
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let rng = OsRng;
        let mut generator = KeystoreJsonGenerator::new(rng, algorithm.clone());
        let encrypted_private_key = generator.generate(password.as_bytes(), private_key_bytes)?;

        // 序列化为字符串
        Ok(wallet_utils::serde_func::serde_to_string(&encrypted_private_key)?)
    }

    pub(crate) async fn get_private_key_with_ctx(
        ctx: &'static Context,
        address: &str,
        chain_code: &str,
    ) -> Result<ChainPrivateKey, crate::error::service::ServiceError> {
        let api_pool = ctx.api_wallet_pool()?;
        let core_pool = ctx.core_pool()?;

        // 查找账户信息
        let account =
            ApiAccountRepo::find_one_by_address_chain_code(address, chain_code, &api_pool)
                .await?
                .ok_or_else(|| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(address.to_string()),
                )
            })?;

        // 获取链信息
        use crate::infrastructure::chain_node::chain_node_ensurer::ChainNodeEnsurer;
        let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool.clone());
        let chain_with_node = ensurer.ensure_and_get_api_chain_with_node(chain_code).await?;

        // 当private_key为None时，动态派生出私钥
        let address_type: AddressType = account.address_type().try_into()?;

        // 调用公共函数生成私钥
        let key = Self::generate_private_key_from_seed_with_ctx(
            ctx,
            &account.wallet_address,
            chain_code,
            &address_type,
            account.account_id,
        )
        .await?;

        // 转换链码用于后续处理
        let code: ChainCode = chain_code.try_into()?;

        // 根据链类型格式化私钥
        let private_key = match code {
            ChainCode::Solana => {
                let keypair = wallet_utils::parse_func::sol_keypair_from_bytes(&key)?;
                keypair.to_base58_string()
            }
            ChainCode::Bitcoin => wallet_chain_interact::btc::wif_private_key(
                &key,
                crate::domain::chain::ChainDomain::network_kind_from_node_network(
                    &chain_with_node.network,
                ),
            )?,
            ChainCode::Dogcoin => wallet_chain_interact::dog::wif_private_key(
                &key,
                crate::domain::chain::ChainDomain::network_kind_from_node_network(
                    &chain_with_node.network,
                ),
            )?,
            ChainCode::Litecoin => wallet_chain_interact::ltc::wif_private_key(
                &key,
                crate::domain::chain::ChainDomain::network_kind_from_node_network(
                    &chain_with_node.network,
                ),
            )?,
            _ => hex::encode(key),
        };

        Ok(private_key.into())
    }

    // pub(crate) async fn decrypt_phrase(
    //     password: &str,
    //     phrase: &str,
    // ) -> Result<String, crate::ServiceError> {
    //     let data = KeystoreJsonDecryptor.decrypt(password.as_ref(), phrase)?;
    //     let phrase = wallet_utils::conversion::vec_to_string(&data)?;
    //     Ok(phrase)
    // }

    pub(crate) async fn derive_subkey_with_ctx(
        ctx: &'static Context,
        uid: &str,
        seed: &[u8],
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        instance: &wallet_chain_instance::instance::ChainObject,
        account_name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
        is_recover: bool,
    ) -> Result<(String, Option<AddressInitReq>), crate::error::service::ServiceError> {
        tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%instance.chain_code(), "ApiAccountDomain: starting derive_subkey");
        let account_name = if is_default_name {
            format!("{account_name}{}", account_index_map.account_id)
        } else {
            account_name.to_string()
        };

        let (address, pubkey, chain_code, derivation_path) = {
            let keypair = instance
                .gen_keypair_with_index_address_type(seed, account_index_map.input_index)
                .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))?;
            (
                keypair.address(),
                keypair.pubkey(),
                keypair.chain_code().to_string(),
                keypair.derivation_path(),
            )
        };

        let address_type = instance.address_type();

        let pool = ctx.api_wallet_pool()?;
        tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: checking if account exists");
        let account = ApiAccountRepo::find_one(
            &pool,
            &address,
            &chain_code,
            &address_type.to_string(),
            api_wallet_type,
        )
        .await?;
        let sn = ctx.get_sn().to_string();

        // 不再需要加密私钥并存储
        let address_type = instance.address_type();
        // let encrypted_private_key_task = EncryptPrivateKeyTask::new(
        //     &address,
        //     address_type,
        //     account_index_map.account_id,
        //     wallet_address,
        //     &chain_code,
        //     api_wallet_type,
        // );
        // Tasks::new().push(CommonTask::EncryptPrivateKey(encrypted_private_key_task)).send().await?;

        let mut req = CreateApiAccountVo::new(
            account_index_map.account_id,
            &address,
            &pubkey,
            wallet_address,
            uid,
            &derivation_path,
            account_index_map.input_index,
            &chain_code,
            &account_name,
            api_wallet_type,
        )
        .with_is_init(is_recover);

        let address_init_req = if let Some(account) = account
            && account.is_init == 1
        {
            tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: account already exists and initialized, skipping init");
            None
        } else {
            tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: account needs init, preparing init request");
            Some(wallet_transport_backend::request::AddressInitReq::new(
                uid,
                &address,
                account_index_map.input_index,
                &instance.chain_code().to_string(),
                &sn,
                vec!["".to_string()],
                &account_name,
            ))
        };

        match address_type {
            AddressType::Btc(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Ltc(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Dog(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Ton(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            _ => {}
        }

        tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: performing DB upsert for account");
        ApiAccountRepo::upsert_account_multi(&pool, vec![req]).await?;
        tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: DB upsert completed successfully");

        // 移除所有副作用：add_account_to_cache 调用
        // 该功能将在异步任务中执行
        tracing::info!(uid=%uid, wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: completed derive_subkey");

        Ok((address, address_init_req))
    }

    /// Fast path for deriving subkey - returns the account data to be inserted
    /// This is used to quickly generate account data for batch insertion
    pub(crate) async fn derive_subkey_fast_with_ctx(
        ctx: &'static Context,
        uid: &str,
        seed: &[u8],
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        instance: &wallet_chain_instance::instance::ChainObject,
        account_name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
        is_recover: bool,
    ) -> Result<
        (String, CreateApiAccountVo, Option<AddressInitReq>),
        crate::error::service::ServiceError,
    > {
        tracing::debug!(wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%instance.chain_code(), "ApiAccountDomain: starting derive_subkey_fast");

        // Derive address from seed and index
        let (address, pubkey, chain_code, derivation_path) = {
            let keypair = instance
                .gen_keypair_with_index_address_type(seed, account_index_map.input_index)
                .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))?;
            (
                keypair.address(),
                keypair.pubkey(),
                keypair.chain_code().to_string(),
                keypair.derivation_path(),
            )
        };

        let address_type = instance.address_type();

        let pool = ctx.api_wallet_pool()?;
        let account = ApiAccountRepo::find_one(
            &pool,
            &address,
            &chain_code,
            &address_type.to_string(),
            api_wallet_type,
        )
        .await?;
        let sn = ctx.get_sn().to_string();
        // Generate account name
        let account_name = if is_default_name {
            format!("{account_name}{}", account_index_map.account_id)
        } else {
            account_name.to_string()
        };

        let mut req = CreateApiAccountVo::new(
            account_index_map.account_id,
            &address,
            &pubkey,
            wallet_address,
            uid,
            &derivation_path,
            account_index_map.input_index,
            &chain_code,
            &account_name,
            api_wallet_type,
        );
        if is_recover {
            req = req.with_is_init(true);
        }

        let address_init_req = if let Some(account) = account
            && account.is_init == 1
        {
            None
        } else {
            Some(wallet_transport_backend::request::AddressInitReq::new(
                uid,
                &address,
                account_index_map.input_index,
                &instance.chain_code().to_string(),
                &sn,
                vec!["".to_string()],
                &account_name,
            ))
        };

        // Set address type if applicable
        match address_type {
            AddressType::Btc(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Ltc(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Dog(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Ton(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            _ => {
                // Do nothing for other address types
            }
        }

        tracing::debug!(wallet_address=%wallet_address, account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chain_code=%chain_code, address=%address, "ApiAccountDomain: completed derive_subkey_fast");

        Ok((address, req, address_init_req))
    }

    pub(crate) async fn address_used_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
        index: i32,
        uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_uid(&pool, uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        let index = wallet_utils::address::AccountIndexMap::from_input_index(index)?;

        let accounts = ApiAccountRepo::find_all_by_wallet_address_index(
            &pool,
            &api_wallet.address,
            chain_code,
            index.account_id,
        )
        .await?;
        for account in accounts {
            ApiAccountRepo::mark_as_used(
                &pool,
                &api_wallet.address,
                account.account_id,
                chain_code,
            )
            .await?;
        }

        Ok(())
    }

    pub async fn get_addresses_with_ctx(
        ctx: &'static Context,
        address: &str,
        account_id: Option<u32>,
        chain_codes: Vec<String>,
    ) -> Result<Vec<AddressChainCode>, ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let mut account_addresses = Vec::new();

        // 获取钱包下的这个账户的所有地址
        tracing::info!(
            address = %address,
            account_id = ?account_id,
            chain_codes = ?chain_codes,
            chain_count = chain_codes.len(),
            "api_wallet.account.get_addresses -> api_account_list"
        );
        let accounts = ApiAccountRepo::api_account_list(
            &pool,
            Some(address.to_string()),
            account_id,
            chain_codes,
        )
        .await?;

        for account in accounts {
            if !account_addresses.iter().any(|address: &AddressChainCode| {
                address.address == account.address && address.chain_code == account.chain_code
            }) {
                account_addresses.push(AddressChainCode {
                    address: account.address,
                    chain_code: account.chain_code,
                });
            }
        }

        tracing::info!("[get addresses] account_addresses: {account_addresses:?}");
        Ok(account_addresses)
    }

    pub(crate) async fn create_sub_account_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        uid: &str,
        password: &str,
        chain_code: &str,
        account_name: &str,
        is_default_name: bool,
        number: u32,
        input_indices: Vec<i32>,
        batch_id: Option<String>,
        is_recover: bool,
    ) -> Result<(), ServiceError> {
        const BATCH_SIZE: usize = 1000;

        let mut done_num = 0;

        for batch in input_indices.chunks(BATCH_SIZE) {
            // 调用核心同步函数：只派生地址 + 写入数据库
            Self::create_api_account_with_ctx(
                ctx,
                wallet_address,
                vec![chain_code.to_string()],
                batch,
                account_name,
                is_default_name,
                ApiWalletType::SubAccount,
                batch_id.clone(),
                is_recover,
                false, // ⭐ 添加：是否最后一页，批量扩容场景设为 false
                0,     // ⭐ 添加：当前页码，批量扩容场景设为 0
            )
            .await?;

            // 异步执行所有副作用，包括通知，不阻塞主流程
            done_num += batch.len() as u32;
        }

        Ok(())
    }

    pub(crate) async fn create_withdrawal_account_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        chains: Vec<String>,
        account_name: &str,
        is_default_name: bool,
        is_recover: bool,
    ) -> Result<(), ServiceError> {
        let chains_for_asset_query = chains.clone();
        Self::create_api_account_with_ctx(
            ctx,
            wallet_address,
            chains,
            &[0, 1],
            account_name,
            is_default_name,
            ApiWalletType::Withdrawal,
            None,
            is_recover,
            false, // ⭐ 添加：是否最后一页，提现账户创建场景设为 false
            0,     // ⭐ 添加：当前页码，提现账户创建场景设为 0
        )
        .await?;

        // 提现账户不会走 QUERY_ADDRESS_LIST，因此需要在这里持久化资产查询任务，
        // 由 asset_query_recovery worker 拉取后调用 query_asset_list 写入余额。
        //
        // 使用 page = -1 避免与地址分页（从 0 开始）冲突。
        const WITHDRAWAL_ASSET_QUERY_PAGE: i64 = -1;
        let pool = ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        let indices = vec![0i32, 1i32];
        let index_list_json = serde_json::to_string(&indices).map_err(|e| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(e.to_string()),
            )
        })?;

        // 仅对确实存在的提现账户链创建任务，避免“链尚未创建账户却把任务置 Done”导致后续无法重试。
        for chain_code in chains_for_asset_query {
            let a1 = ApiAccountRepo::find_all_by_wallet_address_index(
                &pool,
                wallet_address,
                &chain_code,
                1,
            )
            .await?;
            let a2 = ApiAccountRepo::find_all_by_wallet_address_index(
                &pool,
                wallet_address,
                &chain_code,
                2,
            )
            .await?;
            if a1.is_empty() && a2.is_empty() {
                continue;
            }

            AssetQueryStateRepo::upsert_pending(
                &pool,
                &api_wallet.uid,
                &chain_code,
                WITHDRAWAL_ASSET_QUERY_PAGE,
                &index_list_json,
            )
            .await?;
        }

        Ok(())
    }

    pub(crate) async fn create_api_account_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        chains: Vec<String>,
        input_indices: &[i32],
        name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
        batch_id: Option<String>,
        is_recover: bool,
        is_last_page: bool, // ⭐ 添加：是否最后一页
        current_page: i64,  // ⭐ 添加：当前页码
    ) -> Result<Vec<CreateAccountDeferredData>, ServiceError> {
        Self::create_api_account_with_existing_check(
            ctx,
            wallet_address,
            chains,
            input_indices,
            name,
            is_default_name,
            api_wallet_type,
            batch_id,
            is_recover,
            is_last_page,
            current_page,
            ExistingAccountCheck::CheckEach,
        )
        .await
    }

    /// Recover indices that were already compared with the local database in one batch.
    ///
    /// This entry point is intentionally narrow: callers must pass only indices returned by the
    /// backend/local set difference. The account upsert remains the final idempotency guard.
    pub(crate) async fn recover_prefiltered_missing_accounts_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        chain_code: String,
        input_indices: &[i32],
        api_wallet_type: ApiWalletType,
        is_last_page: bool,
        current_page: i64,
    ) -> Result<Vec<CreateAccountDeferredData>, ServiceError> {
        Self::create_api_account_with_existing_check(
            ctx,
            wallet_address,
            vec![chain_code],
            input_indices,
            "账户",
            true,
            api_wallet_type,
            None,
            true,
            is_last_page,
            current_page,
            ExistingAccountCheck::PrefilteredMissing,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_api_account_with_existing_check(
        ctx: &'static Context,
        wallet_address: &str,
        chains: Vec<String>,
        input_indices: &[i32],
        name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
        batch_id: Option<String>,
        is_recover: bool,
        is_last_page: bool,
        current_page: i64,
        existing_account_check: ExistingAccountCheck,
    ) -> Result<Vec<CreateAccountDeferredData>, ServiceError> {
        tracing::info!("➡️ Before core");
        let core_results = Self::create_api_account_core_with_ctx(
            ctx,
            wallet_address,
            chains,
            input_indices,
            name,
            is_default_name,
            api_wallet_type,
            batch_id,
            is_recover,
            is_last_page, // ⭐ 传递：是否最后一页
            current_page, // ⭐ 传递：当前页码
            existing_account_check,
        )
        .await?;
        tracing::info!("⬅️ After core");

        // 发送地址初始化请求和延迟任务到TaskManager
        let mut tasks = Tasks::new();

        // // 发送地址初始化请求（仅当非恢复模式时）
        // if !is_recover {
        //     for core_result in core_results.iter() {
        //         tasks = tasks.push(BackendApiTask::BackendApi(BackendApiTaskData::new(
        //             wallet_transport_backend::consts::endpoint::api_wallet::ADDRESS_INIT,
        //             &core_result.api_address_init_req,
        //         )?));
        //     }
        // }

        // 为每个延迟任务创建Task
        for core_result in core_results.clone() {
            tracing::info!("📌 creating task for chain: {}", core_result.chain_code);
            tasks = tasks.push(CommonTask::CreateApiAccountDeferred(core_result));
        }

        // 发送所有任务到TaskManager
        tasks.send_with_ctx(ctx).await?;
        tracing::info!("⬅️ All tasks sent to TaskManager");
        tracing::info!("⚡ create_api_account returned ok");
        Ok(core_results)
    }

    /// 核心同步执行部分：只处理必须的地址创建逻辑
    /// 绝不能阻塞超过几十毫秒
    async fn create_api_account_core_with_ctx(
        ctx: &'static Context,
        wallet_address: &str,
        chains: Vec<String>,
        input_indices: &[i32],
        name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
        batch_id: Option<String>,
        is_recover: bool,
        is_last_page: bool, // ⭐ 添加：是否最后一页
        _current_page: i64, // ⭐ 添加：当前页码（不再在此处推进地址查询状态）
        existing_account_check: ExistingAccountCheck,
    ) -> Result<Vec<CreateAccountDeferredData>, ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        // 获取种子
        let seed = ApiWalletDomain::new(ctx).get_seed(&api_wallet.address).await?;

        // 遍历每个链，为每个链创建一个延迟任务
        let mut deferred_tasks = Vec::new();

        // 获取当前 epoch，所有任务共用同一个 epoch
        let current_epoch = ConfigDomain::get_keys_reset_epoch(ctx).await?;

        for chain_code in chains {
            let mut created_addresses_for_chain = Vec::new();
            let mut api_account_vo_list_for_chain = Vec::new();
            let mut api_address_init_req = ApiAddressInitReq::new(current_epoch);

            // 遍历每个输入索引，使用 fast path 创建地址数据
            for input_index in input_indices {
                // 构造 index map
                let account_index_map =
                    wallet_utils::address::AccountIndexMap::from_input_index(*input_index)?;

                // 检查索引是否已经存在
                tracing::debug!(wallet_address=%wallet_address, chain_code=%chain_code, account_id=%account_index_map.account_id, "检查索引是否已经存在");
                if existing_account_check.is_required() {
                    let exists = ApiAccountRepo::exists_address(
                        &pool,
                        wallet_address,
                        &chain_code,
                        account_index_map.account_id,
                    )
                    .await?;

                    if exists {
                        tracing::debug!(wallet_address=%wallet_address, chain_code=%chain_code, account_id=%account_index_map.account_id, "索引已存在，跳过");
                        continue;
                    }
                }
                let code: ChainCode = chain_code.as_str().try_into()?;
                let address_types = WalletDomain::address_type_by_chain(code);

                for address_type in address_types {
                    let Ok(node) = ApiChainDomain::get_node(chain_code.as_str(), ctx).await else {
                        tracing::warn!("chain: {:?} node not found", chain_code);
                        continue;
                    };
                    // 获取链实例
                    let instance: wallet_chain_instance::instance::ChainObject = (
                        &code,
                        &address_type,
                        crate::domain::chain::ChainDomain::network_kind_from_node_network(
                            &node.network,
                        ),
                    )
                        .try_into()?;

                    // 使用 fast path 快速生成地址数据
                    let (address, api_account_vo, address_init_req) =
                        Self::derive_subkey_fast_with_ctx(
                            ctx,
                            &api_wallet.uid,
                            &seed,
                            &api_wallet.address,
                            &account_index_map,
                            &instance,
                            name,
                            is_default_name,
                            api_wallet_type,
                            is_recover,
                        )
                        .await?;

                    created_addresses_for_chain.push(address);
                    api_account_vo_list_for_chain.push(api_account_vo);
                    if let Some(address_init_req) = address_init_req {
                        api_address_init_req.address_list.add_address(address_init_req);
                    }
                }
            }

            if let Some(batch_id) = &batch_id
                && !api_address_init_req.address_list.0.is_empty()
            {
                api_address_init_req = api_address_init_req.with_batch_id(batch_id);
            }

            // 批量插入到数据库，减少数据库操作次数
            if !api_account_vo_list_for_chain.is_empty() {
                tracing::info!(wallet_address=%wallet_address, chain_code=%chain_code, count=%api_account_vo_list_for_chain.len(), "批量插入地址数据到数据库");
                ApiAccountRepo::upsert_account_multi(&pool, api_account_vo_list_for_chain).await?;
            }

            // 创建延迟任务
            deferred_tasks.push(CreateAccountDeferredData {
                api_wallet_uid: api_wallet.uid.clone(),
                api_wallet_address: api_wallet.address.clone(),
                created_addresses: created_addresses_for_chain,
                chain_code: chain_code.clone(), // ⭐ 这里填确定的 chain
                api_address_init_req: api_address_init_req,
                is_recover,
                is_last_page, // ⭐ 返回：是否最后一页
            });
        }

        Ok(deferred_tasks)
    }

    /// 延迟执行部分：处理所有副作用
    pub(crate) async fn create_api_account_deferred_with_ctx(
        ctx: &'static Context,
        data: CreateAccountDeferredData,
    ) -> Result<(), ServiceError> {
        tracing::info!("➡️ Before deferred");
        let pool = ctx.api_wallet_pool()?;
        let mut req: TokenQueryPriceReq = TokenQueryPriceReq(Vec::new());
        // let mut all_asset_keys = Vec::new();

        // 获取默认链和币
        let default_coins_list = ApiCoinRepo::coin_list(&pool).await?;

        // 1. 验证 DB 状态
        let accounts = ApiAccountRepo::find_by_addresses(&data.created_addresses, &pool).await?;

        // 如果 DB 中没有找到地址，说明 core 写库失败，中断执行
        if accounts.is_empty() {
            tracing::warn!(uid=%data.api_wallet_uid, "create_api_account_deferred: no accounts found in DB, core may have failed");
            return Ok(());
        }

        // 恢复不需要初始化地址
        // 3. 初始化默认资产并收集资产键
        let mut create_assets = Vec::new();
        for address in &data.created_addresses {
            // 获取地址对应的链信息
            if let Some(account) =
                ApiAccountRepo::find_one_by_address(address, &data.chain_code, &pool).await?
            {
                let asset_vo_list = ApiAssetsDomain::init_default_api_assets(
                    &default_coins_list,
                    address,
                    &account.chain_code,
                    &mut req,
                )
                .await?;
                create_assets.extend(asset_vo_list);
                // all_asset_keys.extend(asset_keys);
            }
        }
        // 3. 批量、幂等写入资产
        //    成功返回即表示本批次资产初始化完成
        ApiAssetsRepo::upsert_assets_multi(&pool, create_assets).await?;

        // 4. 资产初始化完成后，将创建的索引发送到聚合器
        let mut indices: Vec<i32> = accounts.iter().map(|a| a.derivation_path_index).collect();
        indices.sort();
        indices.dedup();

        if !indices.is_empty() {
            let is_withdrawal =
                accounts.iter().any(|a| a.api_wallet_type == ApiWalletType::Withdrawal);
            let page_size =
                if is_withdrawal { WITHDRAWAL_PAGE_SIZE } else { SUB_ACCOUNT_PAGE_SIZE };
            let first_index = indices.first().copied();
            let last_index = indices.last().copied();
            tracing::info!(
                uid = %data.api_wallet_uid,
                chain_code = %data.chain_code,
                indices_count = indices.len(),
                first_index = ?first_index,
                last_index = ?last_index,
                page_size = page_size,
                is_withdrawal = is_withdrawal,
                "Expand notify: enqueue created indices after asset init"
            );
            let msg = PageNotifyMsg::ExpandProgress(ExpandProgressMsg {
                uid: data.api_wallet_uid.clone(),
                chain_code: data.chain_code.clone(),
                wallet_address: data.api_wallet_address.clone(),
                indices,
                page_size,
            });
            let _ = page_notify_tx(ctx).send(msg).await;
        } else {
            tracing::warn!(
                uid = %data.api_wallet_uid,
                chain_code = %data.chain_code,
                "Expand notify: no indices found after asset init, skipping enqueue"
            );
        }

        // // 5. AddressQueryState 处理（is_last_page 仅用于此）
        // if data.is_last_page {
        //     // 是最后一页，标记为 Done
        //     AddressQueryStateRepo::update_status(
        //         &pool,
        //         &data.api_wallet_uid,
        //         &data.chain_code,
        //         AddressQueryStatus::Done,
        //     )
        //     .await?;
        //     tracing::info!(uid=%data.api_wallet_uid, chain_code=%data.chain_code, "Updated AddressQueryStatus to Done");
        // }

        let mut tasks = Tasks::new();
        // 6. 刷新代币价格
        if !req.0.is_empty() {
            tasks = tasks.push(crate::infrastructure::task_queue::CommonTask::QueryCoinPrice(req));
        }

        // 7. 发送地址初始化请求（仅当非恢复模式时）
        if !data.is_recover {
            tasks = tasks.push(BackendApiTask::BackendApi(BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::api_wallet::ADDRESS_INIT,
                &data.api_address_init_req,
            )?));
        }

        // 8. 发送所有后台任务
        // 直接调用 send，不需要检查是否为空，send 方法会处理空的情况
        tasks.send_with_ctx(ctx).await?;

        // // 为每个创建的地址添加到缓存
        // for address in &data.created_addresses {
        //     // 获取地址对应的账户信息
        //     let accounts =
        //         ApiAccountRepo::find_by_addresses(&[address.to_string()], pool.clone()).await?;
        //     for account in accounts {
        //         let _ = asset_calc_actor_manager
        //             .add_account_to_cache(address, account.account_id, &data.api_wallet_address)
        //             .await;
        //     }
        // }

        tracing::info!(uid=%data.api_wallet_uid, "create_api_account_deferred completed");

        Ok(())
    }

    /// 收集当前 uid + chain 下「已经被使用 / 占位」的所有 input_index
    /// used = account ∪ batch_item
    pub async fn collect_used_indices_with_ctx(
        ctx: &'static Context,
        uid: &str,
        chain: &str,
    ) -> Result<std::collections::BTreeSet<i32>, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;

        // 1. account 已初始化的索引
        let account_indices = ApiAccountRepo::get_all_account_indices(&pool, uid, chain).await?;
        tracing::info!(uid=%uid, chain_code=%chain, account_indices=?account_indices, "已初始化的账户索引");
        // 2. batch_item 已占位但未必 init 的索引
        let batch_item_indices =
            ExpandBatchItemRepo::get_all_used_indices(&pool, uid, chain).await?;
        tracing::info!(uid=%uid, chain_code=%chain, batch_item_indices=?batch_item_indices, "已占位但未必初始化的批次索引");

        let mut used = std::collections::BTreeSet::new();

        for account_id in account_indices {
            let idx =
                wallet_utils::address::AccountIndexMap::from_account_id(account_id)?.input_index;
            used.insert(idx);
        }

        for idx in batch_item_indices {
            used.insert(idx);
        }

        Ok(used)
    }

    /// 从 used_indices 中分配 number 个新的 input_index
    /// 保证：
    /// - 不回退
    /// - 不重复
    /// - 不要求连续
    pub fn allocate_indices(used: &std::collections::BTreeSet<i32>, number: u32) -> Vec<i32> {
        let mut result = Vec::with_capacity(number as usize);

        if number == 0 {
            return result;
        }
        let mut candidate = 0;
        while result.len() < number as usize {
            if !used.contains(&candidate) {
                result.push(candidate);
            }
            candidate += 1;
        }

        result
    }

    /// 为批量扩容计算需要分配的索引
    ///
    /// # Arguments
    /// * `uid` - 用户ID
    /// * `chain_code` - 链码
    /// * `batch_id` - 批次ID
    /// * `requested_number` - 请求的索引数量
    ///
    /// # Returns
    /// 返回需要分配的索引列表
    pub(crate) async fn calculate_indices_for_expansion_with_ctx(
        ctx: &'static Context,
        uid: &str,
        chain_code: &str,
        batch_id: &str,
        requested_number: u32,
    ) -> Result<Vec<i32>, crate::error::service::ServiceError> {
        let used = Self::collect_used_indices_with_ctx(ctx, uid, chain_code).await?;

        tracing::info!(
            uid=%uid,
            chain=%chain_code,
            used=?used,
            "已收集所有已使用的索引"
        );

        let pool = ctx.api_wallet_pool()?;
        let batch_item_count = ExpandBatchItemRepo::count_by_batch_id(&pool, batch_id).await?;
        let available_indices = requested_number.saturating_sub(batch_item_count as u32);

        tracing::info!(uid=%uid, chain_code=%chain_code, requested_number=%requested_number, "计算下一批需要扩容的索引");
        let indices = Self::allocate_indices(&used, available_indices);

        if indices.is_empty() {
            tracing::info!(uid=%uid, chain_code=%chain_code, "没有新的索引可分配");
        }
        tracing::info!(uid=%uid, chain_code=%chain_code, final_count=%indices.len(), final_indices=?indices, "完成索引计算，最终需要扩容的索引");

        Ok(indices)
    }

    /// 继续恢复地址查询状态
    /// 从上一个已知状态继续恢复过程
    pub(crate) async fn continue_recover_with_ctx(
        ctx: &'static Context,
        query_state: &AddressQueryStateEntity,
    ) -> Result<(), ServiceError> {
        use crate::infrastructure::task_queue::task::Tasks;

        let pool = ctx.api_wallet_pool()?;

        tracing::info!(uid = %query_state.uid, chain_code = %query_state.chain_code, status = %query_state.status as u8, "继续恢复地址查询状态");

        // 获取现有的地址查询状态，确保幂等性
        let current_state = AddressQueryStateRepo::get_by_uid_and_chain(
            &pool,
            &query_state.uid,
            &query_state.chain_code,
        )
        .await?;

        let start_page =
            Self::resolve_recover_start_page(current_state.as_ref(), query_state.last_page);

        if let Some(s) = current_state.as_ref() {
            if s.status == AddressQueryStatus::Done {
                // 已经完成，直接返回
                tracing::info!(
                    "Address query already done for uid={}, chain_code={}",
                    query_state.uid,
                    query_state.chain_code
                );
                return Ok(());
            } else if s.total_remote == 0 {
                // 没有地址，直接标记为完成
                AddressQueryStateRepo::update_status(
                    &pool,
                    &query_state.uid,
                    &query_state.chain_code,
                    AddressQueryStatus::Done,
                )
                .await?;
                return Ok(());
            }
        }

        // 构造地址列表请求，从数据库里最新已知页码继续
        let address_list_req =
            wallet_transport_backend::request::api_wallet::address::AddressListReq::new(
                &query_state.uid,
                &query_state.chain_code,
                start_page,
                1000,
            );

        // 创建后端API任务数据
        let task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::api_wallet::QUERY_ADDRESS_LIST,
            &address_list_req,
        )?;

        // 使用现有的任务处理机制处理恢复任务
        Tasks::new().push(BackendApiTask::BackendApi(task_data)).send_with_ctx(ctx).await?;

        tracing::info!(uid = %query_state.uid, chain_code = %query_state.chain_code, "地址恢复任务已提交");

        Ok(())
    }
}

#[cfg(test)]
mod test {
    // Test isolation strategy:
    // These tests are pure in-memory unit tests for notify aggregation helpers.
    // They do not depend on DB/network/context and are safe to run in parallel.
    use super::*;
    use alloy::signers::local::PrivateKeySigner;
    use wallet_crypto::{EncryptedJsonDecryptor, KeystoreJsonDecryptor};

    #[test]
    fn prefiltered_missing_indices_skip_per_account_lookup() {
        assert!(!ExistingAccountCheck::PrefilteredMissing.is_required());
        assert!(ExistingAccountCheck::CheckEach.is_required());
    }

    fn mk_chain_state(last_notified_page: i32) -> AddressRecoveryChainState {
        AddressRecoveryChainState {
            loaded_last_notified: true,
            last_notified_page,
            pending_pages: BTreeMap::new(),
            pending_done_number: 0,
            pending_total_number: 0,
            pending_completion: false,
        }
    }

    fn mk_notify_state() -> PageNotifyState {
        PageNotifyState {
            wallet_address: String::new(),
            created_indices_by_chain: HashMap::new(),
            last_notified_page: -1,
            loaded_last_notified: true,
            page_size: SUB_ACCOUNT_PAGE_SIZE,
            dirty: false,
            last_seen_max_page: -1,
            pending_expand_notify_count: 0,
            address_recovery_by_chain: HashMap::new(),
            last_emit_at: None,
        }
    }

    #[test]
    fn fold_pending_recovery_pages_skips_duplicate_page() {
        // Scenario: duplicate page report after restart should not be counted again.
        let mut chain = mk_chain_state(3);
        chain.pending_pages.insert(
            3,
            AddressRecoveryPageState {
                done_number: 1000,
                total_number: 66057,
                is_last_page: false,
            },
        );
        chain.pending_pages.insert(
            4,
            AddressRecoveryPageState { done_number: 57, total_number: 66057, is_last_page: false },
        );

        let advanced = fold_pending_address_recovery_pages(&mut chain);

        assert!(advanced);
        assert_eq!(chain.last_notified_page, 4);
        assert_eq!(chain.pending_done_number, 57);
        assert_eq!(chain.pending_total_number, 66057);
        assert!(!chain.pending_completion);
        assert!(chain.pending_pages.is_empty());
    }

    #[test]
    fn fold_pending_recovery_pages_aggregates_multi_page_once() {
        // Scenario: multiple page updates inside one window should aggregate into one payload.
        let mut chain = mk_chain_state(-1);
        chain.pending_pages.insert(
            0,
            AddressRecoveryPageState {
                done_number: 1000,
                total_number: 65860,
                is_last_page: false,
            },
        );
        chain.pending_pages.insert(
            1,
            AddressRecoveryPageState { done_number: 860, total_number: 65860, is_last_page: false },
        );

        let advanced = fold_pending_address_recovery_pages(&mut chain);

        assert!(advanced);
        assert_eq!(chain.last_notified_page, 1);
        assert_eq!(chain.pending_done_number, 1860);
        assert_eq!(chain.pending_total_number, 65860);
    }

    #[test]
    fn notify_priority_prefers_recovery_when_both_pending() {
        // Scenario: when both ADDRESS_RECOVERY and AWM_CMD_ADDR_EXPAND are pending, recovery wins.
        let mut state = mk_notify_state();
        state.pending_expand_notify_count = 1000;
        let mut chain = mk_chain_state(0);
        chain.pending_done_number = 57;
        chain.pending_total_number = 66057;
        state.address_recovery_by_chain.insert("eth".to_string(), chain);

        let payload = take_pending_address_recovery_notify("u1", &mut state);

        assert!(payload.is_some());
        assert_eq!(state.pending_expand_notify_count, 1000);
        let payload = payload.expect("payload should exist");
        assert_eq!(payload.done_number, 57);
        assert_eq!(payload.number, 66057);
    }

    #[test]
    fn recovery_notify_skips_zero_done_for_non_completion() {
        // Scenario: doneNumber=0 and not completed should not emit ADDRESS_RECOVERY.
        let mut state = mk_notify_state();
        let mut chain = mk_chain_state(10);
        chain.pending_done_number = 0;
        chain.pending_total_number = 66057;
        chain.pending_completion = false;
        state.address_recovery_by_chain.insert("eth".to_string(), chain);

        let payload = take_pending_address_recovery_notify("u1", &mut state);
        assert!(payload.is_none());
    }

    #[test]
    fn emit_window_is_five_seconds() {
        // Scenario: unified notify gate should block emit within 5s and allow at/after 5s.
        let now = Instant::now();
        assert!(can_emit(None, now));
        assert!(!can_emit(Some(now), now + Duration::from_secs(4)));
        assert!(can_emit(Some(now), now + Duration::from_secs(5)));
    }

    #[test]
    fn resolve_recover_start_page_prefers_current_state() {
        let current = AddressQueryStateEntity {
            uid: "uid-1".to_string(),
            chain_code: "tron".to_string(),
            status: AddressQueryStatus::Running,
            last_page: 9,
            total_remote: 10000,
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
        };

        assert_eq!(ApiAccountDomain::resolve_recover_start_page(Some(&current), 2), 10);
    }

    #[test]
    fn resolve_recover_start_page_falls_back_and_clamps_negative_pages() {
        assert_eq!(ApiAccountDomain::resolve_recover_start_page(None, -1), 0);
        assert_eq!(ApiAccountDomain::resolve_recover_start_page(None, 7), 8);

        let current = AddressQueryStateEntity {
            uid: "uid-2".to_string(),
            chain_code: "eth".to_string(),
            status: AddressQueryStatus::Running,
            last_page: -1,
            total_remote: 184485,
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
        };

        assert_eq!(ApiAccountDomain::resolve_recover_start_page(Some(&current), 12), 0);
    }

    async fn test_keystore_key() -> Result<(), Box<dyn std::error::Error>> {
        let key = KeystoreJsonDecryptor.decrypt("q1111111".as_bytes(), r#"{"crypto":{"cipher":"aes-128-ctr","cipherparams":{"iv":"cafaaf94330ae23b8a8eb64660d42740"},"ciphertext":"19e4fee3686f858bc45946665ee751a9964ef956d06ecee2f7a90021bd946529","kdf":"argon2id","kdfparams":{"dklen":32,"time_cost":5,"memory_cost":131072,"parallelism":8,"salt":[63,15,27,159,163,164,60,107,41,155,135,165,52,165,224,219,52,197,122,0,161,45,75,23,49,198,4,140,1,67,182,207]},"mac":"faf334de5be2b30526a8755980372718aad9b477b52753bde820cb6673bba7a9"},"id":"83577d8c-af30-44e6-9f06-5e616b0ac2be","version":3}"#)?;
        let h = hex::encode(key);
        let _: PrivateKeySigner = h.parse().map_err(|_| {
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            )
        })?;
        Ok(())
    }

    #[tokio::test]
    async fn test_keystore() {
        let res = test_keystore_key().await;
        assert!(res.is_ok());
    }
}
