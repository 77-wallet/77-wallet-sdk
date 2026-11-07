use crate::{
    CoreDbPool,
    dao::account::AccountDao,
    entities::account::{
        AccountEntity, AccountWalletMapping, AccountWithWalletEntity, CreateAccountVo,
    },
};
use sqlx::{Sqlite, Transaction};

pub struct AccountRepo;

impl AccountRepo {
    pub async fn list(pool: CoreDbPool) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::account_list_v2(pool.read_ref(), None, None, None, vec![], None).await
    }

    pub async fn get_all_account_indices(pool: CoreDbPool) -> Result<Vec<u32>, crate::Error> {
        AccountDao::get_all_account_indices(pool.read_ref()).await
    }

    pub async fn detail_by_address_and_chain_code(
        pool: CoreDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        AccountDao::detail(pool.read_ref(), None, Some(address), None, Some(chain_code)).await
    }

    pub async fn detail_by_wallet_address_and_account_id_and_chain_code(
        pool: CoreDbPool,
        wallet_address: &str,
        account_id: u32,
        chain_code: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        AccountDao::detail(
            pool.read_ref(),
            Some(wallet_address),
            None,
            Some(account_id),
            Some(chain_code),
        )
        .await
    }

    pub async fn account(
        pool: CoreDbPool,
        address: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        AccountDao::detail(pool.read_ref(), None, Some(address), None, None).await
    }

    pub async fn upsert_multi_account(
        pool: CoreDbPool,
        input: Vec<CreateAccountVo>,
    ) -> Result<(), crate::Error> {
        AccountDao::upsert_multi_account(pool.write_ref(), input).await
    }

    pub async fn account_wallet_mapping(
        pool: CoreDbPool,
    ) -> Result<Vec<AccountWalletMapping>, crate::Error> {
        AccountDao::account_wallet_mapping(pool.read_ref()).await
    }

    pub async fn edit_account_name(
        pool: CoreDbPool,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::edit_account_name(pool.write_ref(), account_id, wallet_address, name).await
    }

    pub async fn account_detail_by_max_id_and_wallet_address(
        pool: CoreDbPool,
        wallet_address: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        AccountDao::account_detail_by_max_id_and_wallet_address(pool.read_ref(), wallet_address)
            .await
    }

    pub async fn has_account_id(
        pool: CoreDbPool,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<bool, crate::Error> {
        AccountDao::has_account_id(pool.read_ref(), wallet_address, account_id).await
    }

    pub async fn account_init(
        pool: CoreDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::init(pool.write_ref(), address, chain_code).await
    }

    pub async fn get_account_list_by_wallet_address(
        pool: CoreDbPool,
        wallet_address: Option<&str>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::account_list_v2(pool.read_ref(), wallet_address, None, None, vec![], None).await
    }

    pub async fn get_account_list_by_wallet_address_and_account_id(
        pool: CoreDbPool,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::account_list_v2(pool.read_ref(), wallet_address, None, None, vec![], account_id)
            .await
    }

    pub async fn account_list_by_wallet_address_and_account_id_and_chain_codes(
        pool: CoreDbPool,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_codes: Vec<String>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::account_list_v2(
            pool.read_ref(),
            wallet_address,
            None,
            None,
            chain_codes,
            account_id,
        )
        .await
    }

    pub async fn account_list_by_wallet_address_and_chain_code(
        pool: CoreDbPool,
        wallet_address: Option<&str>,
        chain_codes: Vec<String>,
        account_id: Option<u32>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::account_list_v2(
            pool.read_ref(),
            wallet_address,
            None,
            None,
            chain_codes,
            account_id,
        )
        .await
    }

    pub async fn lists_by_wallet_address(
        pool: CoreDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::lists_by_wallet_address(wallet_address, account_id, chain_code, pool.read_ref())
            .await
    }

    pub async fn list_in_address(
        pool: CoreDbPool,
        addresses: &[String],
        chain_code: Option<String>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::list_in_address(pool.read_ref(), addresses, chain_code).await
    }

    pub async fn reset(
        pool: CoreDbPool,
        wallet_address: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::reset_account(pool.write_ref(), wallet_address).await
    }

    pub async fn reset_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_address: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::reset_account(tx.as_mut(), wallet_address).await
    }

    pub async fn reset_all_account(pool: CoreDbPool) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::reset_all_account(pool.write_ref()).await
    }

    pub async fn reset_all_account_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::reset_all_account(tx.as_mut()).await
    }

    pub async fn restart(
        pool: CoreDbPool,
        wallet_address: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::restart_account(pool.write_ref(), wallet_address).await
    }

    pub async fn restart_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_address: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::restart_account(tx.as_mut(), wallet_address).await
    }

    pub async fn physical_delete_all(
        pool: CoreDbPool,
        wallet_address: &[&str],
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::physical_delete_all(pool.write_ref(), wallet_address).await
    }

    pub async fn physical_delete_all_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_address: &[&str],
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::physical_delete_all(tx.as_mut(), wallet_address).await
    }

    pub async fn physical_delete(
        pool: CoreDbPool,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::physical_delete(pool.write_ref(), wallet_address, account_id).await
    }

    pub async fn physical_delete_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        AccountDao::physical_delete(tx.as_mut(), wallet_address, account_id).await
    }

    pub async fn count_unique_account_ids(
        pool: CoreDbPool,
        wallet_address: &str,
    ) -> Result<u32, crate::Error> {
        AccountDao::count_unique_account_ids(pool.read_ref(), wallet_address).await
    }

    pub async fn account_with_wallet(
        address: &str,
        chain_code: &str,
        pool: CoreDbPool,
    ) -> Result<AccountWithWalletEntity, crate::Error> {
        AccountDao::account_with_wallet(address, chain_code, pool.read_ref()).await?.ok_or(
            crate::Error::NotFound(format!(
                "account not found: address: {}, chain_code: {}",
                address, chain_code
            )),
        )
    }

    pub async fn current_chain_address(
        address: String,
        account_id: u32,
        chain_code: &str,
        pool: CoreDbPool,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        Ok(AccountDao::current_chain_address(address, account_id, chain_code, pool.read_ref())
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::AccountRepo;
    use crate::repositories::test_helper::{seed_account, seed_wallet, setup_core_pool};

    #[tokio::test]
    async fn account_upsert_and_query_visible() {
        let pool = setup_core_pool("wallet_db_account_success").await;
        let chain_code = wallet_types::constant::chain_code::TRON;
        let wallet_address = "wallet_a1";
        let address = "T_addr_a1";
        seed_wallet(&pool, wallet_address, "uid_a1", "wallet_a1_name").await;

        seed_account(&pool, 1, address, wallet_address, chain_code).await;

        let found =
            AccountRepo::detail_by_address_and_chain_code(pool.clone(), address, chain_code)
                .await
                .unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.address, address);
        assert_eq!(found.chain_code, chain_code);
    }

    #[tokio::test]
    async fn account_query_missing_returns_none() {
        let pool = setup_core_pool("wallet_db_account_edge").await;
        let missing = AccountRepo::detail_by_address_and_chain_code(
            pool,
            "T_addr_not_found",
            wallet_types::constant::chain_code::TRON,
        )
        .await
        .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn account_reset_with_tx_rollback_keeps_data() {
        let pool = setup_core_pool("wallet_db_account_rollback").await;
        let chain_code = wallet_types::constant::chain_code::TRON;
        let wallet_address = "wallet_rb";
        let address = "T_addr_rb";
        seed_wallet(&pool, wallet_address, "uid_rb", "wallet_rb_name").await;
        seed_account(&pool, 9, address, wallet_address, chain_code).await;

        let mut tx = pool.write_ref().begin().await.unwrap();
        let changed = AccountRepo::reset_with_executor(&mut tx, wallet_address).await.unwrap();
        assert!(!changed.is_empty());
        tx.rollback().await.unwrap();

        let found =
            AccountRepo::detail_by_address_and_chain_code(pool, address, chain_code).await.unwrap();
        assert!(found.is_some());
    }
}
