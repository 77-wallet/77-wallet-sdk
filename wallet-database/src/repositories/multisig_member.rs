use crate::{
    CoreDbPool,
    dao::multisig_member::MultisigMemberDaoV1,
    entities::multisig_member::{MultisigMemberEntities, MultisigMemberEntity},
};

pub struct MultisigMemberRepo;

impl MultisigMemberRepo {
    pub async fn list_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::list_by_account_id(account_id, pool.read_ref()).await?)
    }

    pub async fn list_by_uid(
        pool: &CoreDbPool,
        uid: &str,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::list_by_uid(uid, pool.read_ref()).await?)
    }

    pub async fn list_by_uids(
        pool: &CoreDbPool,
        uids: &[String],
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::list_by_uids(uids, pool.read_ref()).await?)
    }

    pub async fn list_by_addresses(
        pool: &CoreDbPool,
        addresses: &[String],
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::list_by_addresses(addresses, pool.read_ref()).await?)
    }

    pub async fn list_by_account_ids_not_addresses(
        pool: &CoreDbPool,
        account_ids: &[String],
        addresses: &[String],
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::list_by_account_ids_not_addresses(
            account_ids,
            addresses,
            pool.read_ref(),
        )
        .await?)
    }

    pub async fn logic_delete_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<(), crate::Error> {
        MultisigMemberDaoV1::logic_del_multisig_member(account_id, pool.write_ref()).await?;
        Ok(())
    }

    pub async fn physical_delete_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<Vec<MultisigMemberEntity>, crate::Error> {
        Ok(MultisigMemberDaoV1::physical_del_multisig_member(account_id, pool.write_ref()).await?)
    }

    pub async fn physical_delete_by_account_ids(
        pool: &CoreDbPool,
        account_ids: &[&str],
    ) -> Result<Vec<MultisigMemberEntity>, crate::Error> {
        Ok(MultisigMemberDaoV1::physical_del_multi_multisig_member(pool.write_ref(), account_ids)
            .await?)
    }

    pub async fn sync_confirmed_and_pubkey_status(
        pool: &CoreDbPool,
        account_id: &str,
        address: &str,
        pubkey: &str,
        status: i8,
        uid: &str,
    ) -> Result<(), crate::Error> {
        MultisigMemberDaoV1::sync_confirmed_and_pubkey_status(
            account_id,
            address,
            pubkey,
            status,
            uid,
            pool.write_ref(),
        )
        .await?;
        Ok(())
    }

    pub async fn find_records_by_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::find_records_by_id(account_id, pool.read_ref()).await?)
    }

    pub async fn get_self_by_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::get_self_by_id(account_id, pool.read_ref()).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::MultisigMemberRepo;
    use crate::{
        dao::multisig_member::MultisigMemberDaoV1, entities::multisig_member::NewMemberEntity,
        repositories::test_helper::setup_core_pool,
    };

    fn build_member(account_id: &str, address: &str, uid: &str) -> NewMemberEntity {
        NewMemberEntity {
            account_id: account_id.to_string(),
            name: "member".to_string(),
            address: address.to_string(),
            pubkey: "pubkey".to_string(),
            confirmed: 0,
            is_self: 1,
            uid: uid.to_string(),
        }
    }

    #[tokio::test]
    async fn multisig_member_repo_list_by_account_id_success() {
        let pool = setup_core_pool("wallet_db_multisig_member_repo_success").await;
        let members = vec![
            build_member("acc_m1", "T_member_1", "uid_1"),
            build_member("acc_m1", "T_member_2", "uid_2"),
        ];
        MultisigMemberDaoV1::batch_add(&members, pool.write_ref()).await.unwrap();

        let listed = MultisigMemberRepo::list_by_account_id(&pool, "acc_m1").await.unwrap();
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn multisig_member_repo_list_by_uid_missing_returns_empty() {
        let pool = setup_core_pool("wallet_db_multisig_member_repo_edge").await;
        let listed = MultisigMemberRepo::list_by_uid(&pool, "uid_missing").await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn multisig_member_repo_tx_rollback_keeps_members_absent() {
        let pool = setup_core_pool("wallet_db_multisig_member_repo_rollback").await;
        let members = vec![build_member("acc_rb", "T_member_rb", "uid_rb")];

        let mut tx = pool.write_ref().begin().await.unwrap();
        MultisigMemberDaoV1::batch_add(&members, tx.as_mut()).await.unwrap();
        tx.rollback().await.unwrap();

        let listed = MultisigMemberRepo::list_by_account_id(&pool, "acc_rb").await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn multisig_member_repo_batch_add_treats_apostrophes_as_data() {
        let pool = setup_core_pool("wallet_db_multisig_member_repo_insert_quote").await;
        let members = vec![build_member("acc_'_quote", "T_'_quote", "uid_'_quote")];

        MultisigMemberDaoV1::batch_add(&members, pool.write_ref()).await.unwrap();

        let listed = MultisigMemberRepo::list_by_account_id(&pool, "acc_'_quote").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].address, "T_'_quote");
        assert_eq!(listed[0].uid, "uid_'_quote");
    }

    #[tokio::test]
    async fn multisig_member_repo_multi_lookups_bind_all_selectors() {
        let pool = setup_core_pool("wallet_db_multisig_member_repo_lookup_bind").await;
        let members = vec![
            build_member("acc_lookup_1", "T_lookup_1", "uid_lookup_1"),
            build_member("acc_lookup_2", "T_lookup_2", "uid_lookup_2"),
        ];
        MultisigMemberDaoV1::batch_add(&members, pool.write_ref()).await.unwrap();
        let sql_shaped = vec!["missing') OR 1=1 --".to_string()];

        let by_uids = MultisigMemberRepo::list_by_uids(&pool, &sql_shaped).await.unwrap();
        let by_addresses = MultisigMemberRepo::list_by_addresses(&pool, &sql_shaped).await.unwrap();
        let by_accounts = MultisigMemberRepo::list_by_account_ids_not_addresses(
            &pool,
            &sql_shaped,
            &["T_lookup_1".to_string()],
        )
        .await
        .unwrap();

        assert!(by_uids.is_empty());
        assert!(by_addresses.is_empty());
        assert!(by_accounts.is_empty());
    }

    #[tokio::test]
    async fn multisig_member_repo_multi_delete_binds_account_ids() {
        let pool = setup_core_pool("wallet_db_multisig_member_repo_delete_bind").await;
        let members = vec![
            build_member("acc_delete_1", "T_delete_1", "uid_delete_1"),
            build_member("acc_delete_2", "T_delete_2", "uid_delete_2"),
        ];
        MultisigMemberDaoV1::batch_add(&members, pool.write_ref()).await.unwrap();

        let deleted =
            MultisigMemberRepo::physical_delete_by_account_ids(&pool, &["missing') OR 1=1 --"])
                .await
                .unwrap();

        assert!(deleted.is_empty());
        assert_eq!(
            MultisigMemberRepo::list_by_account_id(&pool, "acc_delete_1").await.unwrap().len(),
            1
        );
        assert_eq!(
            MultisigMemberRepo::list_by_account_id(&pool, "acc_delete_2").await.unwrap().len(),
            1
        );
    }
}
