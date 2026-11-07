use crate::{
    CoreDbPool,
    dao::{
        account::AccountDao, assets::AssetsDao, multisig_account::MultisigAccountDaoV1,
        multisig_member::MultisigMemberDaoV1, wallet::WalletDao,
    },
    entities::{
        account::AccountEntity,
        coin::CoinMultisigStatus,
        multisig_account::{
            MultisigAccountData, MultisigAccountEntity, MultisigAccountStatus,
            NewMultisigAccountEntity,
        },
        multisig_member::{MemberVo, MultisigMemberEntities},
    },
    pagination::Pagination,
};

pub struct MultisigAccountRepo;

impl MultisigAccountRepo {
    pub fn build_new_account(
        id: Option<String>,
        name: String,
        initiator_addr: String,
        address: String,
        chain_code: String,
        threshold: i32,
        address_type: String,
        member_list: Vec<MemberVo>,
        uids: &std::collections::HashSet<String>,
    ) -> NewMultisigAccountEntity {
        NewMultisigAccountEntity::new(
            id,
            name,
            initiator_addr,
            address,
            chain_code,
            threshold,
            address_type,
            member_list,
            uids,
        )
    }
}

impl MultisigAccountRepo {
    pub async fn account_count(pool: &CoreDbPool, chain_code: &str) -> i64 {
        let account = MultisigAccountDaoV1::account_count(chain_code, pool.read_pool()).await;
        account.unwrap_or_default()
    }

    pub async fn update_name(pool: &CoreDbPool, id: &str, name: &str) -> Result<(), crate::Error> {
        Ok(MultisigAccountDaoV1::update_name(id, name, pool.write_ref()).await?)
    }

    pub async fn cancel_multisig(
        pool: &CoreDbPool,
        account: &MultisigAccountEntity,
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // delete account
        MultisigAccountDaoV1::logic_del_multisig_account(&account.id, tx.as_mut()).await?;

        // recover assets
        if account.chain_code == "tron" {
            AssetsDao::update_tron_multisig_assets(
                &account.address,
                &account.chain_code,
                CoinMultisigStatus::NotMultisig.to_i8(),
                tx.as_mut(),
            )
            .await?;
        }

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn find_by_id(
        pool: &CoreDbPool,
        id: &str,
    ) -> Result<Option<MultisigAccountEntity>, crate::Error> {
        MultisigAccountDaoV1::find_by_id(id, pool.read_ref()).await
    }

    pub async fn find_by_condition(
        pool: &CoreDbPool,
        key: &str,
        value: &str,
    ) -> Result<Option<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::find_by_conditions(vec![(key, value)], pool.read_ref()).await?)
    }

    pub async fn find_by_conditions(
        pool: &CoreDbPool,
        conditions: Vec<(&str, &str)>,
    ) -> Result<Option<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::find_by_conditions(conditions, pool.read_ref()).await?)
    }

    pub async fn update_multisig_address(
        pool: &CoreDbPool,
        multisig_account_id: &str,
        multisig_account_address: &str,
        salt: &str,
        authority_addr: &str,
        address_type: &str,
        deploy_hash: &str,
        fee_hash: &str,
        fee_chain: Option<String>,
    ) -> Result<(), crate::Error> {
        MultisigAccountDaoV1::update_multisig_address(
            multisig_account_id,
            multisig_account_address,
            salt,
            authority_addr,
            address_type,
            deploy_hash,
            fee_hash,
            fee_chain,
            pool.write_ref(),
        )
        .await?;
        Ok(())
    }

    pub async fn pending_account(
        pool: &CoreDbPool,
    ) -> Result<Vec<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::pending_account(pool.read_ref()).await?)
    }

    pub async fn update_status(
        pool: &CoreDbPool,
        id: &str,
        status: Option<i8>,
        pay_status: Option<i8>,
    ) -> Result<(), crate::Error> {
        MultisigAccountDaoV1::update_status(id, status, pay_status, pool.write_ref()).await
    }

    pub async fn sync_status(
        pool: &CoreDbPool,
        id: &str,
        status: MultisigAccountStatus,
    ) -> Result<(), crate::Error> {
        MultisigAccountDaoV1::sync_status(id, status, pool.write_ref()).await?;
        Ok(())
    }

    pub async fn create_account_with_member(
        pool: &CoreDbPool,
        params: &NewMultisigAccountEntity,
    ) -> Result<(), crate::Error> {
        MultisigAccountDaoV1::create_account_with_member(params, pool.write_pool()).await?;
        Ok(())
    }

    pub async fn find_done_account(
        pool: &CoreDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::find_done_account(address, chain_code, pool.read_ref()).await?)
    }

    pub async fn list_all(pool: &CoreDbPool) -> Result<Vec<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::list(vec![], pool.read_ref()).await?)
    }

    pub async fn logic_delete_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<(), crate::Error> {
        MultisigAccountDaoV1::logic_del_multisig_account(account_id, pool.write_ref()).await?;
        Ok(())
    }

    pub async fn delete_in_status(pool: &CoreDbPool, account_id: &str) -> Result<(), crate::Error> {
        MultisigAccountDaoV1::delete_in_status(account_id, pool.write_ref()).await?;
        Ok(())
    }

    pub async fn physical_delete_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<Vec<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::physical_del_multisig_account(account_id, pool.write_ref())
            .await?)
    }

    pub async fn physical_delete_by_account_ids(
        pool: &CoreDbPool,
        account_ids: &[&str],
    ) -> Result<Vec<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::physical_del_multi_multisig_account(pool.write_ref(), account_ids)
            .await?)
    }

    pub async fn update_confirm_status(
        pool: &CoreDbPool,
        account_id: &str,
        chain_code: &str,
        self_address: &mut MultisigMemberEntities,
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        for item in self_address.0.iter_mut() {
            // let req = entities::account::QueryReq::new_address_chain(&item.address, chain_code);

            let account =
                AccountDao::detail(tx.as_mut(), None, Some(&item.address), None, Some(chain_code))
                    .await?
                    .ok_or(crate::DatabaseError::ReturningNone)?;
            // let pubkey = account.map_or_else(|| "".to_string(), |account| account.pubkey);
            let wallet = WalletDao::detail(tx.as_mut(), &account.wallet_address).await?;
            let uid = wallet.map_or_else(|| "".to_string(), |wallet| wallet.uid);

            MultisigMemberDaoV1::sync_confirmed_and_pubkey_status(
                account_id,
                &item.address,
                &account.pubkey,
                1,
                &uid,
                tx.as_mut(),
            )
            .await?;
            item.uid = uid;
            item.pubkey = account.pubkey;
        }

        let member = MultisigMemberDaoV1::find_records_by_id(account_id, tx.as_mut()).await?;
        if member.all_confirmed() {
            MultisigAccountDaoV1::sync_status(
                account_id,
                MultisigAccountStatus::Confirmed,
                tx.as_mut(),
            )
            .await?;
        }

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn create_with_member(
        pool: &CoreDbPool,
        params: &NewMultisigAccountEntity,
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        MultisigAccountDaoV1::insert(params, tx.as_mut()).await?;

        MultisigMemberDaoV1::batch_add(&params.member_list, tx.as_mut()).await?;

        if params.chain_code == "tron" {
            AssetsDao::update_tron_multisig_assets(
                &params.address,
                &params.chain_code,
                CoinMultisigStatus::Deploying.to_i8(),
                tx.as_mut(),
            )
            .await?;
        }

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        Ok(())
    }

    pub async fn account_list(
        pool: &CoreDbPool,
        owner: bool,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigAccountEntity>, crate::Error> {
        let rs = MultisigAccountDaoV1::account_list(
            owner,
            chain_code,
            pool.read_pool(),
            page,
            page_size,
        )
        .await?;
        Ok(rs)
    }

    // 钱包账户
    pub async fn wallet_account(
        pool: &CoreDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        // let req = crate::entities::account::QueryReq::new_address_chain(address, chain_code);

        AccountDao::detail(pool.read_ref(), None, Some(address), None, Some(chain_code)).await
        //     .ok_or(crate::DatabaseError::ReturningNone)?;

        // AccountDao::detail(&*pool, &req).await
    }
    pub async fn update_by_id(
        pool: &CoreDbPool,
        id: &str,
        params: std::collections::HashMap<String, String>,
    ) -> Result<MultisigAccountEntity, crate::Error> {
        Ok(MultisigAccountDaoV1::update_by_id(id, params, pool.write_ref()).await?)
    }

    // get multisig account(include cancel account) and member information
    pub async fn multisig_data(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<MultisigAccountData, crate::Error> {
        // get account
        let conditions = vec![("id", account_id)];

        let account = MultisigAccountDaoV1::find_by_conditions(conditions, pool.read_ref())
            .await?
            .ok_or(crate::DatabaseError::ReturningNone)?;

        let member = MultisigMemberDaoV1::find_records_by_id(account_id, pool.read_ref()).await?;

        Ok(MultisigAccountData::new(account, member))
    }

    pub async fn find_doing_account(
        pool: &CoreDbPool,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<MultisigAccountEntity>, crate::Error> {
        let a =
            MultisigAccountDaoV1::find_doing_account(chain_code, address, pool.read_ref()).await?;
        Ok(a)
    }

    pub async fn pending_handle(
        pool: &CoreDbPool,
        status: MultisigAccountStatus,
    ) -> Result<Vec<MultisigAccountEntity>, crate::Error> {
        Ok(MultisigAccountDaoV1::pending_handle(pool.read_ref(), status).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::MultisigAccountRepo;
    use crate::{
        dao::multisig_account::MultisigAccountDaoV1, entities::multisig_member::MemberVo,
        repositories::test_helper::setup_core_pool,
    };

    #[test]
    fn multisig_account_repo_build_new_account_maps_members_and_self_flag() {
        let mut uids = std::collections::HashSet::new();
        uids.insert("uid-self".to_string());

        let members = vec![
            MemberVo {
                name: "m1".to_string(),
                address: "addr1".to_string(),
                pubkey: "".to_string(),
                confirmed: 0,
                uid: "uid-self".to_string(),
            },
            MemberVo {
                name: "m2".to_string(),
                address: "addr2".to_string(),
                pubkey: "".to_string(),
                confirmed: 0,
                uid: "uid-other".to_string(),
            },
        ];

        let entity = MultisigAccountRepo::build_new_account(
            None,
            "name".to_string(),
            "addr1".to_string(),
            "multi-addr".to_string(),
            "tron".to_string(),
            2,
            "".to_string(),
            members,
            &uids,
        );

        assert_eq!(entity.name, "name");
        assert_eq!(entity.initiator_addr, "addr1");
        assert_eq!(entity.chain_code, "tron");
        assert_eq!(entity.member_num, 2);
        assert_eq!(entity.member_list.len(), 2);
        assert_eq!(entity.member_list[0].is_self, 1);
        assert_eq!(entity.member_list[1].is_self, 0);
    }

    fn build_new_account(
        account_id: &str,
        initiator: &str,
    ) -> crate::entities::multisig_account::NewMultisigAccountEntity {
        let mut uids = std::collections::HashSet::new();
        uids.insert("uid_self".to_string());
        MultisigAccountRepo::build_new_account(
            Some(account_id.to_string()),
            "acc_name".to_string(),
            initiator.to_string(),
            "T_multisig_addr".to_string(),
            "tron".to_string(),
            1,
            "".to_string(),
            vec![MemberVo {
                name: "self".to_string(),
                address: initiator.to_string(),
                pubkey: "pubkey".to_string(),
                confirmed: 1,
                uid: "uid_self".to_string(),
            }],
            &uids,
        )
    }

    #[tokio::test]
    async fn multisig_account_repo_create_and_find_success() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_success").await;
        let params = build_new_account("acc_success", "T_acc_success");

        MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();
        let found = MultisigAccountRepo::find_by_id(&pool, "acc_success").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().initiator_addr, "T_acc_success");
    }

    #[tokio::test]
    async fn multisig_account_repo_find_missing_returns_none() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_edge").await;
        let found = MultisigAccountRepo::find_by_id(&pool, "acc_missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn multisig_account_repo_tx_rollback_keeps_account_absent() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_rollback").await;
        let params = build_new_account("acc_rb", "T_acc_rb");

        let mut tx = pool.write_ref().begin().await.unwrap();
        MultisigAccountDaoV1::insert(&params, tx.as_mut()).await.unwrap();
        tx.rollback().await.unwrap();

        let found = MultisigAccountRepo::find_by_id(&pool, "acc_rb").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn multisig_account_repo_find_by_conditions_ignores_logically_deleted_when_requested() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_deleted_filter").await;
        let params = build_new_account("acc_deleted", "T_acc_deleted");

        MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();
        MultisigAccountRepo::logic_delete_by_account_id(&pool, "acc_deleted").await.unwrap();

        let found_without_is_del =
            MultisigAccountRepo::find_by_condition(&pool, "address", "T_multisig_addr")
                .await
                .unwrap();
        assert!(found_without_is_del.is_some());

        let found_with_is_del = MultisigAccountRepo::find_by_conditions(
            &pool,
            vec![("address", "T_multisig_addr"), ("is_del", "0")],
        )
        .await
        .unwrap();
        assert!(found_with_is_del.is_none());
    }

    #[tokio::test]
    async fn multisig_account_repo_condition_values_are_bound() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_condition_bind").await;
        let mut params = build_new_account("acc_condition", "T_acc_condition");
        params.address = "T_multisig_'_quote".to_string();
        MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();

        let exact = MultisigAccountRepo::find_by_condition(&pool, "address", "T_multisig_'_quote")
            .await
            .unwrap();
        let sql_shaped = "missing' OR 1=1 --";
        let found =
            MultisigAccountRepo::find_by_condition(&pool, "address", sql_shaped).await.unwrap();
        let listed = MultisigAccountDaoV1::list(vec![("address", sql_shaped)], pool.read_ref())
            .await
            .unwrap();

        assert!(exact.is_some());
        assert!(found.is_none());
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn multisig_account_repo_rejects_unknown_dynamic_columns() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_column_guard").await;
        let params = build_new_account("acc_column_guard", "T_column_guard");
        MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();

        let found = MultisigAccountRepo::find_by_condition(
            &pool,
            "address = 'missing' OR 1=1 --",
            "ignored",
        )
        .await;
        let updated = MultisigAccountRepo::update_by_id(
            &pool,
            "acc_column_guard",
            std::collections::HashMap::from([(
                "name = 'changed' WHERE 1=1 --".to_string(),
                "ignored".to_string(),
            )]),
        )
        .await;

        assert!(found.is_err());
        assert!(updated.is_err());
        assert_eq!(
            MultisigAccountRepo::find_by_id(&pool, "acc_column_guard").await.unwrap().unwrap().name,
            "acc_name"
        );
    }

    #[tokio::test]
    async fn multisig_account_repo_account_list_binds_chain_code() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_page_bind").await;
        let params = build_new_account("acc_page_bind", "T_page_bind");
        MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();

        let page =
            MultisigAccountRepo::account_list(&pool, true, Some("missing' OR 1=1 --"), 0, 20)
                .await
                .unwrap();

        assert_eq!(page.total_count, 0);
        assert!(page.data.is_empty());
    }

    #[tokio::test]
    async fn multisig_account_repo_multi_delete_binds_account_ids() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_delete_bind").await;
        for (id, initiator) in
            [("acc_delete_bind_1", "T_delete_bind_1"), ("acc_delete_bind_2", "T_delete_bind_2")]
        {
            let params = build_new_account(id, initiator);
            MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();
        }

        let deleted =
            MultisigAccountRepo::physical_delete_by_account_ids(&pool, &["missing') OR 1=1 --"])
                .await
                .unwrap();

        assert!(deleted.is_empty());
        assert!(
            MultisigAccountRepo::find_by_id(&pool, "acc_delete_bind_1").await.unwrap().is_some()
        );
        assert!(
            MultisigAccountRepo::find_by_id(&pool, "acc_delete_bind_2").await.unwrap().is_some()
        );
    }

    #[tokio::test]
    async fn multisig_account_repo_dynamic_update_and_filtered_page_succeed() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_update_page").await;
        let params = build_new_account("acc_update_page", "T_update_page");
        MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();

        let updated = MultisigAccountRepo::update_by_id(
            &pool,
            "acc_update_page",
            std::collections::HashMap::from([("name".to_string(), "updated_name".to_string())]),
        )
        .await
        .unwrap();
        let page =
            MultisigAccountRepo::account_list(&pool, true, Some("tron"), 0, 20).await.unwrap();

        assert_eq!(updated.name, "updated_name");
        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].id, "acc_update_page");
    }

    #[tokio::test]
    async fn multisig_account_repo_multi_delete_removes_only_selected_accounts() {
        let pool = setup_core_pool("wallet_db_multisig_account_repo_delete_selected").await;
        for (id, initiator) in
            [("acc_delete_selected", "T_delete_selected"), ("acc_delete_keep", "T_delete_keep")]
        {
            let params = build_new_account(id, initiator);
            MultisigAccountRepo::create_account_with_member(&pool, &params).await.unwrap();
        }

        let deleted =
            MultisigAccountRepo::physical_delete_by_account_ids(&pool, &["acc_delete_selected"])
                .await
                .unwrap();

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, "acc_delete_selected");
        assert!(MultisigAccountRepo::find_by_id(&pool, "acc_delete_keep").await.unwrap().is_some());
    }
}
