use once_cell::sync::Lazy;
use wallet_database::entities::api_wallet::ApiWalletEntity;

use crate::{
    config::runtime_defaults,
    error::service::ServiceError,
    infrastructure::protected_query::ProtectedQuery,
    response_vo::api_wallet::wallet::{ApiWalletItem, ApiWalletList, WalletInfo},
};

static API_WALLET_LIST_QUERY: Lazy<ProtectedQuery<ApiWalletList>> =
    Lazy::new(ProtectedQuery::default);

const API_WALLET_LIST_KEY: &str = "api_wallet_list:full";

pub(crate) async fn load_api_wallet_list<F, Fut>(query_fn: F) -> Result<ApiWalletList, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<ApiWalletList, ServiceError>>,
{
    let fresh_ttl = runtime_defaults::api_assets().total_cache_ttl;
    API_WALLET_LIST_QUERY
        .load(API_WALLET_LIST_KEY, fresh_ttl, || async move {
            match query_fn().await {
                Ok(list) => Ok(list),
                Err(err) => {
                    if err.is_transient_db_pool_timeout() {
                        tracing::warn!(
                            metric = "api_wallet_list_db_pool_timeout",
                            err = %err,
                            "api wallet list query hit db pool timeout"
                        );
                    }
                    Err(err)
                }
            }
        })
        .await
}

pub(crate) fn invalidate_api_wallet_list() {
    // 注意：必须用 mark_stale（内部会 invalidate_flight），不能用 mark_key_stale
    // 否则 singleflight 的 state.result 仍保留旧查询结果，下次 load 会跳过 query_fn 直接返回旧值
    API_WALLET_LIST_QUERY
        .mark_stale(API_WALLET_LIST_KEY, runtime_defaults::api_assets().total_cache_ttl);
}

/// 从缓存中移除指定地址的钱包，避免 invalidate 后全量重查 DB。
///
/// 一个列表项可能同时包含充值和提币钱包，因此只清空实际已从 DB 删除的一侧。
pub(crate) fn remove_wallets_from_cache(addresses: &[&str]) {
    if let Some(mut list) = API_WALLET_LIST_QUERY.get_stale(API_WALLET_LIST_KEY) {
        if remove_wallets_from_list(&mut list.0, addresses) {
            API_WALLET_LIST_QUERY.set(API_WALLET_LIST_KEY, &list);
        }
    }
}

fn remove_wallets_from_list(list: &mut Vec<ApiWalletItem>, addresses: &[&str]) -> bool {
    let mut changed = false;
    for item in list.iter_mut() {
        if item
            .recharge_wallet
            .as_ref()
            .is_some_and(|wallet| addresses.contains(&wallet.address.as_str()))
        {
            item.recharge_wallet = None;
            changed = true;
        }
        if item
            .withdraw_wallet
            .as_ref()
            .is_some_and(|wallet| addresses.contains(&wallet.address.as_str()))
        {
            item.withdraw_wallet = None;
            changed = true;
        }
    }
    if changed {
        list.retain(|item| item.recharge_wallet.is_some() || item.withdraw_wallet.is_some());
    }
    changed
}

/// 将新创建/导入的钱包追加到缓存中，避免 invalidate 后全量重查 DB
///
/// push 前用 address 检查是否已存在（DB 层 address UNIQUE），防止重复导入产生重复项。
pub(crate) fn add_wallets_to_cache(wallets: &[ApiWalletEntity]) {
    if wallets.is_empty() {
        return;
    }
    let Some(mut list) = API_WALLET_LIST_QUERY.get_stale(API_WALLET_LIST_KEY) else {
        // Without an existing full snapshot, an incremental write would publish a
        // partial list containing only the newly created/imported wallets.
        invalidate_api_wallet_list();
        return;
    };
    let mut balance_list = std::collections::HashMap::new();
    for item in list.iter() {
        if let Some(r) = &item.recharge_wallet {
            balance_list.insert(r.address.clone(), r.balance.clone());
        }
        if let Some(w) = &item.withdraw_wallet {
            balance_list.insert(w.address.clone(), w.balance.clone());
        }
    }

    use wallet_database::entities::api_wallet::ApiWalletType;
    for e in wallets {
        let mut wallet: WalletInfo = e.into();
        wallet.balance = balance_list.get(&e.address).cloned().unwrap_or_default();
        merge_wallet_into_list(
            &mut list.0,
            wallet,
            e.api_wallet_type,
            e.binding_address.as_deref(),
        );
    }
    API_WALLET_LIST_QUERY.set(API_WALLET_LIST_KEY, &list);
}

/// 4 步匹配逻辑：将钱包合并到 list 中。提取为独立函数以便单测。
///
/// SubAccount 分支：
///   1. recharge address 已存在 → 覆盖（同类型重复导入）
///   2. withdraw address 已存在 → 配对（跨类型同地址）
///   3. binding_address 匹配 → 找配对的 Withdrawal
///   4. push 新项
///
/// Withdrawal 分支：
///   1. withdraw address 已存在 → 覆盖（同类型重复导入）
///   2. recharge address 已存在 → 配对（跨类型同地址）
///   3. binding_address 匹配 → 找配对的 SubAccount
///   4. push 新项
pub(crate) fn merge_wallet_into_list(
    list: &mut Vec<ApiWalletItem>,
    wallet: WalletInfo,
    wallet_type: wallet_database::entities::api_wallet::ApiWalletType,
    binding_address: Option<&str>,
) {
    use wallet_database::entities::api_wallet::ApiWalletType;
    match wallet_type {
        ApiWalletType::SubAccount => {
            if let Some(item) = list.iter_mut().find(|item| {
                item.recharge_wallet.as_ref().map(|r| r.address == wallet.address).unwrap_or(false)
            }) {
                item.recharge_wallet = Some(wallet);
            } else if let Some(item) = list.iter_mut().find(|item| {
                item.withdraw_wallet.as_ref().map(|w| w.address == wallet.address).unwrap_or(false)
            }) {
                item.recharge_wallet = Some(wallet);
            } else if let Some(ba) = binding_address
                && let Some(item) = list.iter_mut().find(|item| {
                    item.withdraw_wallet.as_ref().map(|w| &w.address == ba).unwrap_or(false)
                })
            {
                item.recharge_wallet = Some(wallet);
            } else {
                list.push(ApiWalletItem { recharge_wallet: Some(wallet), withdraw_wallet: None });
            }
        }
        ApiWalletType::Withdrawal => {
            if let Some(item) = list.iter_mut().find(|item| {
                item.withdraw_wallet.as_ref().map(|w| w.address == wallet.address).unwrap_or(false)
            }) {
                item.withdraw_wallet = Some(wallet);
            } else if let Some(item) = list.iter_mut().find(|item| {
                item.recharge_wallet.as_ref().map(|r| r.address == wallet.address).unwrap_or(false)
            }) {
                item.withdraw_wallet = Some(wallet);
            } else if let Some(ba) = binding_address
                && let Some(item) = list.iter_mut().find(|item| {
                    item.recharge_wallet.as_ref().map(|r| &r.address == ba).unwrap_or(false)
                })
            {
                item.withdraw_wallet = Some(wallet);
            } else {
                list.push(ApiWalletItem { recharge_wallet: None, withdraw_wallet: Some(wallet) });
            }
        }
    }
}

/// 绑定后将缓存中两个独立 item 合并成一个
pub(crate) fn merge_wallets_in_cache(recharge_address: &str, withdrawal_address: &str) {
    let mut list: ApiWalletList = API_WALLET_LIST_QUERY
        .get_stale(API_WALLET_LIST_KEY)
        .unwrap_or_else(|| ApiWalletList(Vec::new()));

    let mut recharge_wallet = None;
    let mut withdraw_wallet = None;

    // 提取独立的 recharge item 和 withdraw item，移除原项
    list.0.retain(|item| {
        if let Some(r) = &item.recharge_wallet
            && r.address == recharge_address
            && item.withdraw_wallet.is_none()
        {
            recharge_wallet = Some(r.clone());
            return false;
        }
        if let Some(w) = &item.withdraw_wallet
            && w.address == withdrawal_address
            && item.recharge_wallet.is_none()
        {
            withdraw_wallet = Some(w.clone());
            return false;
        }
        true
    });

    if let (Some(r), Some(w)) = (recharge_wallet, withdraw_wallet) {
        list.0.push(ApiWalletItem { recharge_wallet: Some(r), withdraw_wallet: Some(w) });
        API_WALLET_LIST_QUERY.set(API_WALLET_LIST_KEY, &list);
    } else {
        // 缓存中没有找到对应的独立 item，走 mark_stale 让下次读取重查 DB
        invalidate_api_wallet_list();
    }
}

/// 精准更新缓存中指定钱包的名字，避免 invalidate 后全量重查 DB
pub(crate) fn rename_wallet_in_cache(address: &str, new_name: &str) {
    if let Some(mut list) = API_WALLET_LIST_QUERY.get_stale(API_WALLET_LIST_KEY) {
        let mut changed = false;
        for item in list.iter_mut() {
            if let Some(r) = item.recharge_wallet.as_mut()
                && r.address == address
            {
                r.name = new_name.to_string();
                changed = true;
            }
            if let Some(w) = item.withdraw_wallet.as_mut()
                && w.address == address
            {
                w.name = new_name.to_string();
                changed = true;
            }
        }
        if changed {
            API_WALLET_LIST_QUERY.set(API_WALLET_LIST_KEY, &list);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::time::{Duration, sleep};

    use crate::{
        error::service::ServiceError, infrastructure::protected_query::ProtectedQuery,
        response_vo::api_wallet::wallet::ApiWalletList,
    };

    fn unique_key(prefix: &str) -> String {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        format!("{prefix}-{}", ts.as_nanos())
    }

    #[tokio::test]
    async fn api_wallet_list_policy_deduplicates_entrypoint_loads() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..32 {
            let hits = hits.clone();
            tasks.push(tokio::spawn(async move {
                super::load_api_wallet_list(|| async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    sleep(Duration::from_millis(20)).await;
                    Ok(ApiWalletList::new())
                })
                .await
            }));
        }

        for task in tasks {
            let res = task.await.expect("join ok").expect("list query ok");
            assert!(res.is_empty());
        }

        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn api_wallet_list_query_deduplicates_concurrent_loads() {
        let protected = Arc::new(ProtectedQuery::<ApiWalletList>::default());
        let key = unique_key("api-wallet-list");
        let hits = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..32 {
            let protected = protected.clone();
            let key = key.clone();
            let hits = hits.clone();
            tasks.push(tokio::spawn(async move {
                protected
                    .load(&key, Duration::from_secs(5), || async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        sleep(Duration::from_millis(20)).await;
                        Ok(ApiWalletList::new())
                    })
                    .await
            }));
        }

        for task in tasks {
            let res = task.await.expect("join ok").expect("list query ok");
            assert!(res.is_empty());
        }

        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn api_wallet_list_query_returns_stale_on_timeout() {
        let protected = ProtectedQuery::<ApiWalletList>::default();
        let key = unique_key("api-wallet-list-stale");

        let first = protected
            .load(&key, Duration::from_secs(60), || async { Ok(ApiWalletList::new()) })
            .await
            .expect("initial query ok");
        assert!(first.is_empty());

        protected.invalidate_flight(&key);
        protected.mark_key_stale(&key, Duration::from_secs(3600));

        let stale = protected
            .load(&key, Duration::from_millis(0), || async { Err(ServiceError::Timeout) })
            .await
            .expect("timeout should return stale list");
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn api_wallet_list_policy_drops_deleted_wallet_after_invalidation() {
        // 使用局部 ProtectedQuery + unique_key 避免全局缓存污染，精确控制 TTL 行为
        let protected = ProtectedQuery::<ApiWalletList>::default();
        let key = unique_key("api-wallet-list-invalidate");
        let ttl = Duration::from_secs(60);
        let hits = Arc::new(AtomicUsize::new(0));

        // 1. 首次加载：填充缓存（1 个 item）
        let hits1 = hits.clone();
        let cached = protected
            .load(&key, ttl, move || async move {
                hits1.fetch_add(1, Ordering::SeqCst);
                Ok(ApiWalletList(vec![crate::response_vo::api_wallet::wallet::ApiWalletItem {
                    recharge_wallet: None,
                    withdraw_wallet: None,
                }]))
            })
            .await
            .expect("initial list query ok");
        assert_eq!(cached.len(), 1);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // 2. mark stale（与 invalidate_api_wallet_list 内部实现相同语义：mark stale + invalidate flight）
        protected.mark_stale(&key, ttl);
        // 验证 stale 标记后 get_fresh 取不到（TTL 过期），但 get_stale 仍能取到旧内容
        assert!(
            protected.get_fresh(&key, ttl).is_none(),
            "after mark stale, get_fresh should miss"
        );
        assert!(protected.get_stale(&key).is_some(), "after mark stale, get_stale should hit");

        // 3. 再次加载：TTL 已过期，应重新执行 query_fn，返回新的空列表覆盖旧缓存
        let hits2 = hits.clone();
        let reloaded = protected
            .load(&key, ttl, move || async move {
                hits2.fetch_add(1, Ordering::SeqCst);
                Ok(ApiWalletList::new())
            })
            .await
            .expect("reloaded list query ok");

        assert_eq!(hits.load(Ordering::SeqCst), 2, "query_fn should have been called twice");
        assert!(
            reloaded.is_empty(),
            "reloaded should be empty after new query, got len={}",
            reloaded.len()
        );

        // 4. mark stale 后，失败时仍能 fallback 到上一次成功写入的 stale 数据（空 list）
        protected.mark_stale(&key, ttl);
        let stale = protected
            .load(&key, Duration::from_millis(0), || async { Err(ServiceError::Timeout) })
            .await
            .expect("timeout should return stale empty list");
        assert!(stale.is_empty());
    }

    fn make_wallet(
        address: &str,
        name: &str,
    ) -> crate::response_vo::api_wallet::wallet::WalletInfo {
        crate::response_vo::api_wallet::wallet::WalletInfo {
            address: address.to_string(),
            uid: format!("uid_{}", address),
            name: name.to_string(),
            app_id: None,
            sn: None,
            balance: Default::default(),
        }
    }

    fn make_wallet_entity(address: &str) -> wallet_database::entities::api_wallet::ApiWalletEntity {
        use sqlx::types::chrono::Utc;
        use wallet_database::entities::api_wallet::ApiWalletType;

        wallet_database::entities::api_wallet::ApiWalletEntity {
            id: 1,
            name: "wallet".to_string(),
            uid: format!("uid_{address}"),
            address: address.to_string(),
            phrase: Vec::new(),
            seed: Vec::new(),
            binding_address: None,
            api_wallet_type: ApiWalletType::SubAccount,
            merchant_id: None,
            app_id: None,
            sn: None,
            status: 1,
            is_init: 1,
            import_stage: 0,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    fn make_bound_wallet_entity(
        address: &str,
        wallet_type: wallet_database::entities::api_wallet::ApiWalletType,
        binding_address: &str,
        app_id: &str,
    ) -> wallet_database::entities::api_wallet::ApiWalletEntity {
        let mut entity = make_wallet_entity(address);
        entity.api_wallet_type = wallet_type;
        entity.binding_address = Some(binding_address.to_string());
        entity.app_id = Some(app_id.to_string());
        entity
    }

    #[test]
    fn remove_withdrawal_wallet_preserves_paired_recharge_wallet() {
        let mut list = vec![crate::response_vo::api_wallet::wallet::ApiWalletItem {
            recharge_wallet: Some(make_wallet("recharge", "merchant")),
            withdraw_wallet: Some(make_wallet("withdrawal", "merchant")),
        }];

        let changed = super::remove_wallets_from_list(&mut list, &["withdrawal"]);

        assert!(changed);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].recharge_wallet.as_ref().unwrap().address, "recharge");
        assert!(list[0].withdraw_wallet.is_none());
    }

    #[test]
    fn remove_both_wallets_drops_empty_pair() {
        let mut list = vec![crate::response_vo::api_wallet::wallet::ApiWalletItem {
            recharge_wallet: Some(make_wallet("recharge", "merchant")),
            withdraw_wallet: Some(make_wallet("withdrawal", "merchant")),
        }];

        let changed = super::remove_wallets_from_list(&mut list, &["recharge", "withdrawal"]);

        assert!(changed);
        assert!(list.is_empty());
    }

    #[test]
    fn add_wallets_without_full_cache_does_not_publish_partial_list() {
        super::API_WALLET_LIST_QUERY.invalidate_key(super::API_WALLET_LIST_KEY);

        super::add_wallets_to_cache(&[make_wallet_entity("new_wallet")]);

        assert!(super::API_WALLET_LIST_QUERY.get_stale(super::API_WALLET_LIST_KEY).is_none());
    }

    #[test]
    fn refreshing_bound_pair_replaces_stale_recharge_app_id() {
        use wallet_database::entities::api_wallet::ApiWalletType;

        let mut list = vec![crate::response_vo::api_wallet::wallet::ApiWalletItem {
            recharge_wallet: Some(make_wallet("recharge", "merchant")),
            withdraw_wallet: None,
        }];
        let recharge = make_bound_wallet_entity(
            "recharge",
            ApiWalletType::SubAccount,
            "withdrawal",
            "bound-app-id",
        );
        let withdrawal = make_bound_wallet_entity(
            "withdrawal",
            ApiWalletType::Withdrawal,
            "recharge",
            "bound-app-id",
        );

        for entity in [&recharge, &withdrawal] {
            super::merge_wallet_into_list(
                &mut list,
                entity.into(),
                entity.api_wallet_type,
                entity.binding_address.as_deref(),
            );
        }

        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].recharge_wallet.as_ref().and_then(|wallet| wallet.app_id.as_deref()),
            Some("bound-app-id")
        );
        assert_eq!(
            list[0].withdraw_wallet.as_ref().and_then(|wallet| wallet.app_id.as_deref()),
            Some("bound-app-id")
        );
    }

    #[test]
    fn merge_cross_type_same_address_merges_instead_of_duplicating() {
        use wallet_database::entities::api_wallet::ApiWalletType;

        // 场景：先导入 SubAccount(addr=X)，再导入 Withdrawal(addr=X)
        // 期望：只有 1 个 item，包含 recharge 和 withdraw
        let mut list = Vec::new();
        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户1"),
            ApiWalletType::SubAccount,
            None,
        );
        assert_eq!(list.len(), 1);
        assert!(list[0].recharge_wallet.is_some());
        assert!(list[0].withdraw_wallet.is_none());

        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户11"),
            ApiWalletType::Withdrawal,
            None,
        );
        assert_eq!(list.len(), 1, "should merge, not duplicate");
        assert!(list[0].recharge_wallet.is_some());
        assert!(list[0].withdraw_wallet.is_some());
        assert_eq!(list[0].recharge_wallet.as_ref().unwrap().name, "商户1");
        assert_eq!(list[0].withdraw_wallet.as_ref().unwrap().name, "商户11");
    }

    #[test]
    fn merge_same_type_same_address_updates_existing() {
        use wallet_database::entities::api_wallet::ApiWalletType;

        // 场景：重复导入相同 SubAccount（不同 name）
        let mut list = Vec::new();
        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户1"),
            ApiWalletType::SubAccount,
            None,
        );
        assert_eq!(list.len(), 1);

        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户11"),
            ApiWalletType::SubAccount,
            None,
        );
        assert_eq!(list.len(), 1, "should update, not duplicate");
        assert_eq!(list[0].recharge_wallet.as_ref().unwrap().name, "商户11");
    }

    #[test]
    fn merge_cross_type_reverse_order_merges() {
        use wallet_database::entities::api_wallet::ApiWalletType;

        // 场景：先导入 Withdrawal(addr=X)，再导入 SubAccount(addr=X)
        let mut list = Vec::new();
        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户11"),
            ApiWalletType::Withdrawal,
            None,
        );
        assert_eq!(list.len(), 1);
        assert!(list[0].withdraw_wallet.is_some());
        assert!(list[0].recharge_wallet.is_none());

        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户1"),
            ApiWalletType::SubAccount,
            None,
        );
        assert_eq!(list.len(), 1, "should merge, not duplicate");
        assert!(list[0].recharge_wallet.is_some());
        assert!(list[0].withdraw_wallet.is_some());
    }

    #[test]
    fn merge_different_addresses_create_separate_items() {
        use wallet_database::entities::api_wallet::ApiWalletType;

        let mut list = Vec::new();
        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "商户1"),
            ApiWalletType::SubAccount,
            None,
        );
        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_Y", "商户2"),
            ApiWalletType::Withdrawal,
            None,
        );
        assert_eq!(list.len(), 2, "different addresses should create separate items");
    }

    #[test]
    fn merge_binding_address_matches_pairing() {
        use wallet_database::entities::api_wallet::ApiWalletType;

        // 场景：先导入 Withdrawal(addr=Y)，再导入 SubAccount(addr=X, binding=Y)
        // SubAccount 应通过 binding_address 找到 Withdrawal 并配对
        let mut list = Vec::new();
        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_Y", "提币钱包"),
            ApiWalletType::Withdrawal,
            None,
        );
        assert_eq!(list.len(), 1);

        crate::domain::api_wallet::wallet_list_policy::merge_wallet_into_list(
            &mut list,
            make_wallet("addr_X", "子账户钱包"),
            ApiWalletType::SubAccount,
            Some("addr_Y"),
        );
        assert_eq!(list.len(), 1, "binding_address should pair them");
        assert!(list[0].recharge_wallet.is_some());
        assert!(list[0].withdraw_wallet.is_some());
    }
}
