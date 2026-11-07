use crate::{
    ApiWalletDbPool,
    dao::api_account::{ApiAccountDao, ApiAccountEntitySummer, ApiAccountSummeryEntity},
    db::sqlite_retry::with_sqlite_locked_retry,
    entities::{
        account::AccountEntity,
        api_account::{
            AccountToWalletAddress, ApiAccountAddressPageRow, ApiAccountEntity,
            ApiAccountWalletMapping, CreateApiAccountVo,
        },
        api_wallet::ApiWalletType,
    },
};

pub struct ApiAccountRepo;

impl ApiAccountRepo {
    pub async fn account_high_watermark(pool: &ApiWalletDbPool) -> Result<i64, crate::Error> {
        ApiAccountDao::account_high_watermark(pool.read_ref()).await
    }

    pub async fn missing_chain_account_page(
        pool: &ApiWalletDbPool,
        chain: &str,
        after_id: i64,
        through_id: i64,
        limit: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        ApiAccountDao::missing_chain_account_page(
            pool.read_ref(),
            chain,
            after_id,
            through_id,
            limit,
        )
        .await
    }

    pub async fn missing_chain_account_page_for_variants(
        pool: &ApiWalletDbPool,
        chain: &str,
        after_id: i64,
        through_id: i64,
        limit: u32,
        expected_variants: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        ApiAccountDao::missing_chain_account_page_for_variants(
            pool.read_ref(),
            chain,
            after_id,
            through_id,
            limit,
            expected_variants,
        )
        .await
    }

    pub async fn target_uninitialized_for_logical(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: u32,
        api_wallet_type: ApiWalletType,
        chain: &str,
        limit: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        ApiAccountDao::target_uninitialized_for_logical(
            pool.read_ref(),
            wallet_address,
            account_id,
            api_wallet_type,
            chain,
            limit,
        )
        .await
    }

    pub async fn list_active_address_page_by_wallet(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        after_id: i64,
        limit: u32,
    ) -> Result<Vec<ApiAccountAddressPageRow>, crate::Error> {
        ApiAccountDao::list_active_address_page_by_wallet(
            pool.read_ref(),
            wallet_address,
            account_id,
            after_id,
            limit,
        )
        .await
    }

    async fn with_write_guard<T, F, Fut>(
        pool: &ApiWalletDbPool,
        op: &'static str,
        action: F,
    ) -> Result<T, crate::Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::Error>>,
    {
        let _write_guard = pool.lock_write_with_metric(op).await;
        let tx_start = std::time::Instant::now();
        let result = with_sqlite_locked_retry(action).await;
        let elapsed_ms = tx_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            metric = "write_tx_duration_ms",
            db = "api_wallet.db",
            op,
            value_ms = %elapsed_ms,
            ok = %result.is_ok(),
            "api account write finished"
        );
        result
    }

    pub async fn find_one(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
        address_type: &str,
        api_wallet_type: ApiWalletType,
    ) -> Result<Option<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::find_one(
            pool.read_ref(),
            address,
            chain_code,
            address_type,
            api_wallet_type,
        )
        .await?)
    }

    pub async fn upsert_account_multi(
        pool: &ApiWalletDbPool,
        input: Vec<CreateApiAccountVo>,
    ) -> Result<(), crate::Error> {
        Self::with_write_guard(pool, "upsert_account_multi", || async {
            let mut tx =
                pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
            ApiAccountDao::upsert_multi(tx.as_mut(), input.clone()).await?;
            tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
            Ok(())
        })
        .await
    }

    pub async fn list_inited_indices(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        chain_code: &str,
    ) -> Result<Vec<(i32,)>, crate::Error> {
        Ok(ApiAccountDao::list_inited_indices(pool.read_ref(), wallet_address, chain_code).await?)
    }

    pub async fn list_inited_indices_by_candidates(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        chain_code: &str,
        candidates: &[i32],
    ) -> Result<Vec<(i32,)>, crate::Error> {
        Ok(ApiAccountDao::list_inited_indices_by_candidates(
            pool.read_ref(),
            wallet_address,
            chain_code,
            candidates,
        )
        .await?)
    }

    pub async fn mark_as_used(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: u32,
        chain_code: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Self::with_write_guard(pool, "mark_as_used", || async {
            Ok(ApiAccountDao::update_is_used(
                pool.write_ref(),
                wallet_address,
                account_id,
                chain_code,
                true,
            )
            .await?)
        })
        .await
    }

    pub async fn get_all_account_indices(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<u32>, crate::Error> {
        Ok(ApiAccountDao::get_all_account_indices(pool.read_ref(), uid, chain_code).await?)
    }

    pub async fn init(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Self::with_write_guard(pool, "init_account", || async {
            Ok(ApiAccountDao::init(pool.write_ref(), address, chain_code).await?)
        })
        .await
    }

    pub async fn init_many(
        pool: &ApiWalletDbPool,
        pairs: &[(String, String)],
    ) -> Result<u64, crate::Error> {
        Self::with_write_guard(pool, "init_many_accounts", || async {
            Ok(ApiAccountDao::init_many(pool.write_ref(), pairs).await?)
        })
        .await
    }

    pub async fn expand(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Self::with_write_guard(pool, "expand_account", || async {
            Ok(ApiAccountDao::expand(pool.write_ref(), address, chain_code).await?)
        })
        .await
    }

    pub async fn delete(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Self::with_write_guard(pool, "delete_account", || async {
            Ok(ApiAccountDao::physical_delete(pool.write_ref(), wallet_address, account_id).await?)
        })
        .await
    }

    pub async fn api_account_list(
        pool: &ApiWalletDbPool,
        wallet_address: Option<String>,
        account_id: Option<u32>,
        chain_codes: Vec<String>,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::api_account_list(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_codes,
        )
        .await?)
    }

    pub async fn find_all_by_wallet_address_index(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        chain_code: &str,
        account_id: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::find_all_by_wallet_address_index(
            pool.read_ref(),
            wallet_address,
            chain_code,
            account_id,
        )
        .await?)
    }

    pub async fn has_account_id(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: u32,
        api_wallet_type: ApiWalletType,
    ) -> Result<bool, crate::Error> {
        Ok(ApiAccountDao::has_account_id(
            pool.read_ref(),
            wallet_address,
            account_id,
            api_wallet_type,
        )
        .await?)
    }

    pub async fn account_detail_by_max_id_and_wallet_address(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        api_wallet_type: ApiWalletType,
    ) -> Result<Option<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::account_detail_by_max_id_and_wallet_address(
            pool.read_ref(),
            wallet_address,
            api_wallet_type,
        )
        .await?)
    }

    pub async fn find_one_by_address_chain_code(
        address: &str,
        chain_code: &str,
        exec: &ApiWalletDbPool,
    ) -> Result<Option<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::find_one_by_address_chain_code(address, chain_code, exec.read_ref())
            .await?)
    }

    pub async fn list_by_wallet_address(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::lists_by_wallet_address(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
        )
        .await?)
    }

    pub async fn count_by_wallet_address(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<i64, crate::Error> {
        ApiAccountDao::count_by_wallet_address(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
        )
        .await
    }

    pub async fn count_by_wallet_address_up_to(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
        limit: i64,
    ) -> Result<i64, crate::Error> {
        ApiAccountDao::count_by_wallet_address_up_to(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
            limit,
        )
        .await
    }

    pub async fn list(pool: &ApiWalletDbPool) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        ApiAccountDao::account_list(pool.read_ref(), None, None, None, vec![], None).await
    }

    pub async fn list_by_wallet_address_account_id(
        pool: &ApiWalletDbPool,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        ApiAccountDao::account_list(pool.read_ref(), wallet_address, None, None, vec![], account_id)
            .await
    }

    pub async fn account_wallet_mapping(
        pool: &ApiWalletDbPool,
        api_wallet_type: Option<ApiWalletType>,
    ) -> Result<Vec<ApiAccountWalletMapping>, crate::Error> {
        ApiAccountDao::account_wallet_mapping(pool.read_ref(), api_wallet_type).await
    }

    pub async fn find_one_by_address(
        address: &str,
        chain_code: &str,
        exec: &ApiWalletDbPool,
    ) -> Result<Option<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::find_one_by_address(address, chain_code, exec.read_ref()).await?)
    }

    /// 地址搜索：在指定 API 钱包 uid 范围内搜索地址，支持大小写不敏感匹配
    pub async fn search_address_by_uid(
        pool: &ApiWalletDbPool,
        uid: &str,
        keyword: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::search_address_by_uid(pool.read_ref(), uid, keyword).await?)
    }

    /// 批量查询账户（通过地址列表）
    pub async fn find_by_addresses(
        addresses: &[String],
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::find_by_addresses(addresses, pool.read_ref()).await?)
    }

    pub async fn find_one_by_wallet_address_account_id_chain_code(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: u32,
        chain_code: &str,
    ) -> Result<Option<ApiAccountEntity>, crate::Error> {
        Ok(ApiAccountDao::find_one_by_wallet_address_account_id_chain_code(
            wallet_address,
            account_id,
            chain_code,
            pool.read_ref(),
        )
        .await?)
    }

    pub async fn edit_account_name(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: u32,
        name: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        Ok(ApiAccountDao::edit_account_name(pool.write_ref(), wallet_address, account_id, name)
            .await?)
    }

    pub async fn account_to_wallet(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<AccountToWalletAddress>, crate::Error> {
        ApiAccountDao::account_to_wallet(pool.read_ref()).await
    }

    pub async fn physical_delete_all(
        pool: &ApiWalletDbPool,
        wallet_addresses: &[&str],
    ) -> Result<Vec<ApiAccountEntity>, crate::Error> {
        ApiAccountDao::physical_delete_all(pool.write_ref(), wallet_addresses).await
    }

    pub async fn count_unique_account_ids(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
    ) -> Result<u32, crate::Error> {
        ApiAccountDao::count_unique_account_ids(pool.read_ref(), wallet_address).await
    }

    pub async fn lists_by_wallet_address_v2(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<ApiAccountSummeryEntity>, crate::Error> {
        ApiAccountDao::lists_by_wallet_address_v2(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
            page,
            page_size,
        )
        .await
    }

    pub async fn count_by_status(pool: &ApiWalletDbPool, status: i32) -> Result<i64, crate::Error> {
        Ok(ApiAccountDao::count_by_status(pool.read_ref(), status).await?)
    }

    pub async fn exists_by_chain_code(
        pool: &ApiWalletDbPool,
        chain_code: &str,
    ) -> Result<bool, crate::Error> {
        Ok(ApiAccountDao::exists_by_chain_code(pool.read_ref(), chain_code).await?)
    }

    pub async fn count_by_wallet_address_v2(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
    ) -> Result<i64, crate::Error> {
        ApiAccountDao::count_by_wallet_address_v2(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
        )
        .await
    }

    pub async fn count_by_wallet_address_v3(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
    ) -> Result<i64, crate::Error> {
        ApiAccountDao::count_by_wallet_address_v3(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
        )
        .await
    }

    pub async fn lists_by_wallet_address_v3(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_ids: Vec<u32>,
        chain_code: Option<String>,
    ) -> Result<Vec<ApiAccountSummeryEntity>, crate::Error> {
        ApiAccountDao::lists_by_wallet_address_v3(
            pool.read_ref(),
            wallet_address,
            account_ids,
            chain_code,
        )
        .await
    }
    pub async fn lists_acc_by_wallet_address_v3(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<ApiAccountEntitySummer>, crate::Error> {
        ApiAccountDao::lists_acc_by_wallet_address_v3(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
            page,
            page_size,
        )
        .await
    }

    /// 检查指定的 wallet_address、chain_code 和 account_id 是否存在
    pub async fn exists_address(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        chain_code: &str,
        account_id: u32,
    ) -> Result<bool, crate::Error> {
        Ok(ApiAccountDao::exists_address(pool.read_ref(), wallet_address, chain_code, account_id)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiAccountRepo;
    use crate::{
        dao::api_account::ApiAccountDao,
        entities::{api_account::CreateApiAccountVo, api_wallet::ApiWalletType},
        repositories::test_helper::setup_api_wallet_pool,
    };

    fn make_account_vo(
        account_id: u32,
        address: &str,
        wallet_address: &str,
        chain_code: &str,
    ) -> CreateApiAccountVo {
        CreateApiAccountVo::new(
            account_id,
            address,
            "pubkey",
            wallet_address,
            "uid_account_test",
            "m/44'/60'/0'/0/0",
            0,
            chain_code,
            "acc_name",
            ApiWalletType::SubAccount,
        )
    }

    #[tokio::test]
    async fn account_repo_upsert_and_find_success() {
        let pool = setup_api_wallet_pool("wallet_db_api_account_success").await;
        let wallet_address = "0xapi_account_wallet_s";
        let address = "0xapi_account_addr_s";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;

        let vo = make_account_vo(1, address, wallet_address, chain_code);
        ApiAccountRepo::upsert_account_multi(&pool, vec![vo]).await.unwrap();

        let got = ApiAccountRepo::find_one_by_wallet_address_account_id_chain_code(
            &pool,
            wallet_address,
            1,
            chain_code,
        )
        .await
        .unwrap();
        let got = got.unwrap();
        assert_eq!(got.account_id, 1);
        assert_eq!(got.address, address);
        assert_eq!(got.wallet_address, wallet_address);
        assert_eq!(got.chain_code, chain_code);
        assert!(!got.is_used);

        let count =
            ApiAccountRepo::count_by_wallet_address(&pool, wallet_address, None, Some(chain_code))
                .await
                .unwrap();
        assert_eq!(count, 1);

        let list = ApiAccountRepo::list_by_wallet_address(&pool, wallet_address, None, None)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].address, address);
    }

    #[tokio::test]
    async fn account_repo_missing_account_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_api_account_edge").await;
        let got = ApiAccountRepo::find_one_by_wallet_address_account_id_chain_code(
            &pool,
            "0xapi_account_wallet_missing",
            99,
            wallet_types::constant::chain_code::ETHEREUM,
        )
        .await
        .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn count_by_wallet_address_up_to_stops_at_limit() {
        let pool = setup_api_wallet_pool("wallet_db_api_account_bounded_count").await;
        let wallet_address = "0xapi_account_wallet_count";
        let other_wallet = "0xapi_account_wallet_count_other";

        let accounts = vec![
            make_account_vo(1, "0xcount_eth_1", wallet_address, "eth"),
            make_account_vo(2, "0xcount_eth_2", wallet_address, "eth"),
            make_account_vo(3, "0xcount_eth_3", wallet_address, "eth"),
            make_account_vo(4, "0xcount_bnb_1", wallet_address, "bnb"),
            make_account_vo(5, "0xcount_bnb_2", wallet_address, "bnb"),
            make_account_vo(6, "0xcount_inactive", wallet_address, "eth"),
            make_account_vo(1, "0xcount_other", other_wallet, "eth"),
        ];
        ApiAccountRepo::upsert_account_multi(&pool, accounts).await.unwrap();
        sqlx::query("UPDATE api_account SET status = 0 WHERE address = ?")
            .bind("0xcount_inactive")
            .execute(pool.write_ref())
            .await
            .unwrap();

        let capped =
            ApiAccountRepo::count_by_wallet_address_up_to(&pool, wallet_address, None, None, 3)
                .await
                .unwrap();
        assert_eq!(capped, 3);

        let below_limit =
            ApiAccountRepo::count_by_wallet_address_up_to(&pool, wallet_address, None, None, 10)
                .await
                .unwrap();
        assert_eq!(below_limit, 5);

        let chain_filtered = ApiAccountRepo::count_by_wallet_address_up_to(
            &pool,
            wallet_address,
            None,
            Some("bnb"),
            10,
        )
        .await
        .unwrap();
        assert_eq!(chain_filtered, 2);
    }

    #[tokio::test]
    async fn account_repo_pages_active_wallet_addresses_by_stable_id_cursor() {
        let pool = setup_api_wallet_pool("wallet_db_api_account_address_cursor").await;
        let wallet_address = "0xapi_account_wallet_cursor";
        let other_wallet = "0xapi_account_wallet_other";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;

        let accounts = vec![
            make_account_vo(1, "0xcursor_1", wallet_address, chain_code),
            make_account_vo(1, "0xcursor_2", wallet_address, chain_code),
            make_account_vo(2, "0xcursor_3", wallet_address, chain_code),
            make_account_vo(1, "0xcursor_other", other_wallet, chain_code),
        ];
        ApiAccountRepo::upsert_account_multi(&pool, accounts).await.unwrap();
        sqlx::query("UPDATE api_account SET status = 0 WHERE address = ?")
            .bind("0xcursor_2")
            .execute(pool.write_ref())
            .await
            .unwrap();

        let first =
            ApiAccountRepo::list_active_address_page_by_wallet(&pool, wallet_address, None, 0, 1)
                .await
                .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].address, "0xcursor_1");

        let second = ApiAccountRepo::list_active_address_page_by_wallet(
            &pool,
            wallet_address,
            None,
            first[0].id,
            2,
        )
        .await
        .unwrap();
        assert_eq!(
            second.iter().map(|row| row.address.as_str()).collect::<Vec<_>>(),
            vec!["0xcursor_3"]
        );

        let account_filtered = ApiAccountRepo::list_active_address_page_by_wallet(
            &pool,
            wallet_address,
            Some(2),
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(account_filtered.len(), 1);
        assert_eq!(account_filtered[0].address, "0xcursor_3");

        let empty = ApiAccountRepo::list_active_address_page_by_wallet(
            &pool,
            wallet_address,
            None,
            second[0].id,
            10,
        )
        .await
        .unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn account_repo_list_acc_page_zero_returns_first_page() {
        let pool = setup_api_wallet_pool("wallet_db_api_account_page_zero").await;
        let wallet_address = "0xapi_account_wallet_page_zero";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;

        for account_id in 1..=12u32 {
            let address = format!("0xapi_account_addr_{account_id:02}");
            let vo = make_account_vo(account_id, &address, wallet_address, chain_code);
            ApiAccountRepo::upsert_account_multi(&pool, vec![vo]).await.unwrap();
        }

        let first_page = ApiAccountRepo::lists_acc_by_wallet_address_v3(
            &pool,
            wallet_address,
            None,
            Some(chain_code.to_string()),
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(first_page.len(), 10);
        assert_eq!(first_page[0].account_id, 1);
        assert_eq!(first_page[9].account_id, 10);

        let second_page = ApiAccountRepo::lists_acc_by_wallet_address_v3(
            &pool,
            wallet_address,
            None,
            Some(chain_code.to_string()),
            1,
            10,
        )
        .await
        .unwrap();
        assert_eq!(second_page.len(), 2);
        assert_eq!(second_page[0].account_id, 11);
        assert_eq!(second_page[1].account_id, 12);
    }

    #[tokio::test]
    async fn account_repo_tx_rollback_keeps_is_used_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_api_account_rollback").await;
        let wallet_address = "0xapi_account_wallet_rb";
        let address = "0xapi_account_addr_rb";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;

        let vo = make_account_vo(2, address, wallet_address, chain_code);
        ApiAccountRepo::upsert_account_multi(&pool, vec![vo]).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        let changed =
            ApiAccountDao::update_is_used(tx.as_mut(), wallet_address, 2, chain_code, true)
                .await
                .unwrap();
        assert!(!changed.is_empty());
        tx.rollback().await.unwrap();

        let got = ApiAccountRepo::find_one_by_wallet_address_account_id_chain_code(
            &pool,
            wallet_address,
            2,
            chain_code,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!got.is_used);

        let count =
            ApiAccountRepo::count_by_wallet_address(&pool, wallet_address, None, Some(chain_code))
                .await
                .unwrap();
        assert_eq!(count, 1);
    }
}

#[cfg(test)]
#[path = "account/missing_chain_tests.rs"]
mod missing_chain_tests;
