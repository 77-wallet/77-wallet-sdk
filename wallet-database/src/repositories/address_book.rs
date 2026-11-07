use crate::{
    CoreDbPool, dao::address_book::AddressBookDao, entities::address_book::AddressBookEntity,
    pagination::Pagination,
};

pub struct AddressBookRepo;

impl AddressBookRepo {
    pub async fn insert(
        pool: &CoreDbPool,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::insert(pool.write_ref(), name, address, chain_code).await?)
    }

    pub async fn update(
        pool: &CoreDbPool,
        id: u32,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::update(pool.write_ref(), id, name, address, chain_code).await?)
    }

    pub async fn find_by_conditions(
        pool: &CoreDbPool,
        conditions: Vec<(&str, &str)>,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::find_condition(pool.read_ref(), conditions).await?)
    }

    pub async fn check_not_self(
        pool: &CoreDbPool,
        id: u32,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::check_not_self(pool.read_ref(), id, address, chain_code).await?)
    }

    pub async fn delete(pool: &CoreDbPool, id: i32) -> Result<(), crate::Error> {
        Ok(AddressBookDao::delete(pool.write_ref(), id).await?)
    }

    pub async fn list(
        pool: &CoreDbPool,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::list(pool.read_pool(), chain_code, page, page_size).await?)
    }

    pub async fn find_by_address(
        pool: &CoreDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::find_by_address(pool.read_ref(), address, chain_code).await?)
    }

    pub async fn find_by_address_chain(
        pool: &CoreDbPool,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::find_by_address(pool.read_ref(), address, chain_code).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::AddressBookRepo;
    use crate::{dao::address_book::AddressBookDao, repositories::test_helper::setup_core_pool};

    #[tokio::test]
    async fn address_book_insert_update_delete_success() {
        let pool = setup_core_pool("wallet_db_address_book_success").await;
        let chain_code = wallet_types::constant::chain_code::TRON;
        let address = "T_addr_book_1";

        let inserted =
            AddressBookRepo::insert(&pool, "name_1", address, chain_code).await.unwrap().unwrap();
        let updated =
            AddressBookRepo::update(&pool, inserted.id as u32, "name_2", address, chain_code)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(updated.name, "name_2");

        let found = AddressBookRepo::find_by_address(&pool, address, chain_code).await.unwrap();
        assert!(found.is_some());

        AddressBookRepo::delete(&pool, inserted.id).await.unwrap();
        let deleted = AddressBookRepo::find_by_address(&pool, address, chain_code).await.unwrap();
        assert!(deleted.is_none());
    }

    #[tokio::test]
    async fn address_book_query_missing_returns_none() {
        let pool = setup_core_pool("wallet_db_address_book_edge").await;
        let found = AddressBookRepo::find_by_address(
            &pool,
            "T_addr_book_not_exist",
            wallet_types::constant::chain_code::TRON,
        )
        .await
        .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn address_book_list_filters_chain_and_counts_rows() {
        let pool = setup_core_pool("wallet_db_address_book_list").await;
        AddressBookRepo::insert(&pool, "tron", "T_list", "tron").await.unwrap();
        AddressBookRepo::insert(&pool, "eth", "0x_list", "eth").await.unwrap();

        let page = AddressBookRepo::list(&pool, Some("tron"), 0, 10).await.unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].chain_code, "tron");
    }

    #[tokio::test]
    async fn address_book_list_treats_sql_shaped_chain_as_data() {
        let pool = setup_core_pool("wallet_db_address_book_list_bind").await;
        AddressBookRepo::insert(&pool, "tron", "T_bind", "tron").await.unwrap();
        AddressBookRepo::insert(&pool, "eth", "0x_bind", "eth").await.unwrap();

        let page = AddressBookRepo::list(&pool, Some("tron' OR 1=1 --"), 0, 10).await.unwrap();

        assert_eq!(page.total_count, 0);
        assert!(page.data.is_empty());
    }

    #[tokio::test]
    async fn address_book_find_conditions_binds_values_and_rejects_unknown_columns() {
        let pool = setup_core_pool("wallet_db_address_book_conditions_bind").await;
        AddressBookRepo::insert(&pool, "owner's wallet", "T_conditions", "tron").await.unwrap();

        let exact = AddressBookRepo::find_by_conditions(
            &pool,
            vec![("name", "owner's wallet"), ("chain_code", "tron")],
        )
        .await
        .unwrap();
        assert_eq!(exact.unwrap().address, "T_conditions");

        let injected =
            AddressBookRepo::find_by_conditions(&pool, vec![("name", "missing' OR 1=1 --")])
                .await
                .unwrap();
        assert!(injected.is_none());

        let invalid_column =
            AddressBookRepo::find_by_conditions(&pool, vec![("name OR 1=1 --", "ignored")]).await;
        assert!(matches!(
            invalid_column,
            Err(crate::Error::Database(crate::DatabaseError::QueryFailed))
        ));
    }

    #[tokio::test]
    async fn address_book_tx_rollback_keeps_no_residue() {
        let pool = setup_core_pool("wallet_db_address_book_rollback").await;
        let chain_code = wallet_types::constant::chain_code::TRON;
        let address = "T_addr_book_rb";

        let mut tx = pool.write_ref().begin().await.unwrap();
        let inserted =
            AddressBookDao::insert(tx.as_mut(), "name_rb", address, chain_code).await.unwrap();
        assert!(inserted.is_some());
        tx.rollback().await.unwrap();

        let found = AddressBookRepo::find_by_address(&pool, address, chain_code).await.unwrap();
        assert!(found.is_none());
    }
}
