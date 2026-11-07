use crate::{
    CoreDbPool,
    dao::bill::BillDao,
    entities::{
        asset_token_key::AssetTokenKey,
        bill::{BillEntity, BillKind, BillUpdateEntity, NewBillEntity, RecentBillListVo},
    },
    pagination::Pagination,
};
use serde::Serialize;
use sqlx::{Executor, Sqlite};

pub struct BillRepo;

impl BillRepo {
    pub fn build_signed_bill(hash: String, from: String, chain_code: String) -> NewBillEntity {
        NewBillEntity::new_signed_bill(hash, from, chain_code)
    }

    pub fn build_bill(
        hash: String,
        from: String,
        to: String,
        value: f64,
        chain_code: String,
        symbol: String,
        multisig_tx: bool,
        tx_kind: BillKind,
        notes: String,
    ) -> NewBillEntity {
        Self::build_bill_with_extra::<String>(
            hash,
            from,
            to,
            value,
            chain_code,
            symbol,
            multisig_tx,
            tx_kind,
            notes,
        )
    }

    pub fn build_bill_with_extra<T>(
        hash: String,
        from: String,
        to: String,
        value: f64,
        chain_code: String,
        symbol: String,
        multisig_tx: bool,
        tx_kind: BillKind,
        notes: String,
    ) -> NewBillEntity<T>
    where
        T: Serialize,
    {
        let tx_type = if tx_kind.in_transfer_type() { 0 } else { 1 };

        NewBillEntity {
            hash,
            from,
            to,
            token: AssetTokenKey::Native,
            value,
            multisig_tx,
            symbol,
            chain_code,
            tx_type,
            tx_kind,
            status: 1,
            queue_id: String::new(),
            notes,
            transaction_fee: "0".to_string(),
            resource_consume: String::new(),
            transaction_time: 0,
            block_height: "0".to_string(),
            signer: vec![],
            extra: None,
        }
    }

    pub fn build_deploy_bill(
        hash: String,
        initiator_addr: String,
        chain_code: String,
        symbol: String,
    ) -> NewBillEntity {
        NewBillEntity::new_deploy_bill(hash, initiator_addr, chain_code, symbol)
    }

    pub fn build_bill_update(
        hash: String,
        format_fee: String,
        time: u128,
        status: i8,
        block_height: u128,
        resource_consume: String,
    ) -> BillUpdateEntity {
        BillUpdateEntity::new(hash, format_fee, time, status, block_height, resource_consume)
    }

    pub fn build_stake_bill<T: Serialize>(
        hash: String,
        from: String,
        to: String,
        value: f64,
        bill_kind: BillKind,
        bill_consumer: String,
        transaction_fee: String,
        extra: Option<T>,
    ) -> NewBillEntity<T> {
        NewBillEntity::new_stake_bill(
            hash,
            from,
            to,
            value,
            bill_kind,
            bill_consumer,
            transaction_fee,
            extra,
        )
    }

    pub fn build_bill_from<T, R>(input: T) -> R
    where
        R: From<T>,
    {
        R::from(input)
    }

    pub fn try_build_bill_from<T, R, E>(input: T) -> Result<R, E>
    where
        R: TryFrom<T, Error = E>,
    {
        R::try_from(input)
    }
}

impl BillRepo {
    pub async fn create<T>(tx: NewBillEntity<T>, pool: &CoreDbPool) -> Result<(), crate::Error>
    where
        T: Serialize,
    {
        BillDao::create(tx, pool.write_ref()).await
    }

    pub async fn update_all<T>(
        pool: &CoreDbPool,
        tx: NewBillEntity<T>,
        id: i32,
    ) -> Result<(), crate::Error>
    where
        T: Serialize,
    {
        BillDao::update_all(pool.into_inner(), tx, id).await
    }

    pub async fn last_bill(
        pool: &CoreDbPool,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<BillEntity>, crate::Error> {
        Ok(BillDao::last_bill(chain_code, address, pool.read_ref()).await?)
    }

    // 获取交易
    pub async fn get_by_hash_and_owner(
        tx_hash: &str,
        owner: &str,
        pool: &CoreDbPool,
    ) -> Result<BillEntity, crate::Error> {
        let bill = BillDao::get_by_hash_and_owner(pool.read_ref(), tx_hash, owner).await?.ok_or(
            crate::Error::NotFound(format!(
                "bill not found,tx_hash = {} ,owenr = {}",
                tx_hash, owner,
            )),
        )?;

        Ok(bill)
    }

    pub async fn get_by_hash_opt(
        hash: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<BillEntity>, crate::Error> {
        let bill = BillDao::get_one_by_hash(hash, pool.read_ref()).await?;

        Ok(bill)
    }

    pub async fn get_by_hash_and_type(
        hash: &str,
        transfer_type: i64,
        pool: &CoreDbPool,
    ) -> Result<Option<BillEntity>, crate::Error> {
        BillDao::get_by_hash_and_type(pool.read_ref(), hash, transfer_type).await
    }

    pub async fn find_by_id(id: &str, pool: &CoreDbPool) -> Result<BillEntity, crate::Error> {
        let bill = BillDao::find_by_id(pool.read_ref(), id)
            .await?
            .ok_or(crate::Error::NotFound(format!("bill not found,id = {}", id,)))?;

        Ok(bill)
    }

    pub async fn lists_by_hashs(
        owner: &str,
        hashs: Vec<String>,
        pool: &CoreDbPool,
    ) -> Result<Vec<BillEntity>, crate::Error> {
        BillDao::lists_by_hashs(pool.read_ref(), owner, hashs).await
    }

    pub async fn recent_bill(
        token: &str,
        addr: &str,
        chain_code: &str,
        page: i64,
        page_size: i64,
        pool: CoreDbPool,
    ) -> Result<Pagination<RecentBillListVo>, crate::Error> {
        let min_value = None;
        let lists = BillDao::recent_bill(
            token,
            addr,
            chain_code,
            min_value,
            page,
            page_size,
            pool.into_inner(),
        )
        .await?;

        Ok(lists)
    }

    pub async fn update<'a, E>(
        transaction: &BillUpdateEntity,
        tx: E,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        BillDao::update(transaction, tx).await
    }

    pub async fn update_fail(tx_hash: &str, pool: &CoreDbPool) -> Result<(), crate::Error> {
        BillDao::update_fail(tx_hash, pool.write_ref()).await?;

        Ok(())
    }

    pub async fn bill_lists(
        addr: &[String],
        chain_code: Option<&str>,
        symbol: Option<&str>,
        is_multisig: Option<i64>,
        min_value: Option<f64>,
        start: Option<i64>,
        end: Option<i64>,
        transfer_type: Vec<i32>,
        page: i64,
        page_size: i64,
        pool: &CoreDbPool,
    ) -> Result<Pagination<BillEntity>, crate::Error> {
        let lists = BillDao::bill_lists(
            pool.read_ref(),
            addr,
            chain_code,
            symbol,
            is_multisig,
            min_value,
            start,
            end,
            transfer_type,
            page,
            page_size,
        )
        .await?;
        Ok(lists)
    }

    pub async fn last_swap_bill(
        from: &str,
        chain_code: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<BillEntity>, crate::Error> {
        BillDao::last_swap_bill(pool.read_ref(), from, chain_code).await
    }

    pub async fn last_approve_bill(
        from: &str,
        to: &str,
        contract: &str,
        chain_code: &str,
        tx_kind: BillKind,
        pool: &CoreDbPool,
    ) -> Result<Option<BillEntity>, crate::Error> {
        BillDao::last_approve_bill(pool.read_ref(), from, to, contract, chain_code, tx_kind).await
    }

    pub async fn bill_count(pool: &CoreDbPool) -> Result<i64, crate::Error> {
        BillDao::bill_count(pool.read_ref()).await
    }

    pub async fn on_going_bill(
        chain_code: &str,
        address: &str,
        pool: &CoreDbPool,
    ) -> Result<Vec<BillEntity>, crate::Error> {
        BillDao::on_going_bill(chain_code, address, pool.read_ref()).await
    }

    pub async fn last_kind_bill(
        owner_address: &str,
        bill_kind: Vec<i8>,
        pool: &CoreDbPool,
    ) -> Result<Option<BillEntity>, crate::Error> {
        BillDao::last_kind_bill(pool.read_ref(), owner_address, bill_kind).await
    }

    pub async fn first_transfer(
        address: &str,
        chain_code: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<BillEntity>, crate::Error> {
        Ok(BillDao::first_transfer(address, chain_code, pool.read_ref()).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::BillRepo;
    use crate::{
        dao::bill::BillDao,
        entities::bill::{BillKind, BillStatus},
        repositories::test_helper::setup_core_pool,
    };

    fn make_temp_dir(prefix: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    #[tokio::test]
    async fn bill_repo_create_and_get_by_hash_opt_success() {
        let dir = make_temp_dir("wallet_db_bill_repo_happy");
        let ctx = crate::SqliteContext::new(&dir, Some("data.db")).await.unwrap();
        let pool = ctx.into_core_db_pool().unwrap();

        let mut bill = BillRepo::build_bill(
            "tx_hash_1".to_string(),
            "from_addr".to_string(),
            "to_addr".to_string(),
            1.0,
            wallet_types::constant::chain_code::TRON.to_string(),
            "TRX".to_string(),
            false,
            BillKind::Transfer,
            "test".to_string(),
        );
        bill.status = BillStatus::Pending.to_i8();

        BillRepo::create(bill, &pool).await.unwrap();
        let found = BillRepo::get_by_hash_opt("tx_hash_1", &pool).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().hash, "tx_hash_1");
    }

    #[tokio::test]
    async fn bill_repo_create_preserves_apostrophes_in_text_fields() {
        let pool = setup_core_pool("wallet_db_bill_repo_apostrophe").await;
        let mut bill = BillRepo::build_bill(
            "tx_hash_quote".to_string(),
            "from'address".to_string(),
            "to'address".to_string(),
            1.0,
            wallet_types::constant::chain_code::TRON.to_string(),
            "TRX".to_string(),
            false,
            BillKind::Transfer,
            "recipient's note".to_string(),
        );
        bill.status = BillStatus::Pending.to_i8();

        BillRepo::create(bill, &pool).await.unwrap();

        let stored = BillRepo::get_by_hash_opt("tx_hash_quote", &pool)
            .await
            .unwrap()
            .expect("bill should be stored");
        assert_eq!(stored.from_addr, "from'address");
        assert_eq!(stored.to_addr, "to'address");
        assert_eq!(stored.notes, "recipient's note");
    }

    #[tokio::test]
    async fn bill_repo_last_kind_bill_accepts_apostrophe_in_owner() {
        let pool = setup_core_pool("wallet_db_bill_last_kind_apostrophe").await;
        let bill = BillRepo::build_bill(
            "last_kind_hash".to_string(),
            "owner's address".to_string(),
            "recipient".to_string(),
            1.0,
            wallet_types::constant::chain_code::TRON.to_string(),
            "TRX".to_string(),
            false,
            BillKind::Transfer,
            String::new(),
        );
        BillRepo::create(bill, &pool).await.unwrap();

        let stored =
            BillRepo::last_kind_bill("owner's address", vec![BillKind::Transfer.to_i8()], &pool)
                .await
                .unwrap()
                .expect("matching bill should be returned");

        assert_eq!(stored.hash, "last_kind_hash");
    }

    #[tokio::test]
    async fn bill_repo_lists_by_hashs_accepts_apostrophes() {
        let pool = setup_core_pool("wallet_db_bill_hash_list_apostrophe").await;
        let bill = BillRepo::build_bill(
            "hash'with'quotes".to_string(),
            "owner's address".to_string(),
            "recipient".to_string(),
            1.0,
            wallet_types::constant::chain_code::TRON.to_string(),
            "TRX".to_string(),
            false,
            BillKind::Transfer,
            String::new(),
        );
        BillRepo::create(bill, &pool).await.unwrap();

        let stored = BillRepo::lists_by_hashs(
            "owner's address",
            vec!["hash'with'quotes".to_string()],
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].hash, "hash'with'quotes");
    }

    #[tokio::test]
    async fn bill_repo_bill_lists_binds_text_filters_and_counts_matches() {
        let pool = setup_core_pool("wallet_db_bill_lists_apostrophe").await;
        let bill = BillRepo::build_bill(
            "bill_lists_hash".to_string(),
            "owner's address".to_string(),
            "recipient".to_string(),
            1.0,
            "tron'network".to_string(),
            "TR'X".to_string(),
            false,
            BillKind::Transfer,
            String::new(),
        );
        BillRepo::create(bill, &pool).await.unwrap();

        let page = BillRepo::bill_lists(
            &["owner's address".to_string()],
            Some("tron'network"),
            Some("TR'X"),
            Some(0),
            None,
            None,
            None,
            vec![BillKind::Transfer.to_i8() as i32],
            0,
            10,
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].hash, "bill_lists_hash");
    }

    #[tokio::test]
    async fn bill_dao_recent_bill_binds_text_filters() {
        let pool = setup_core_pool("wallet_db_recent_bill_apostrophe").await;
        let bill = BillRepo::build_bill(
            "recent_bill_hash".to_string(),
            "owner's address".to_string(),
            "recipient's address".to_string(),
            1.0,
            "tron'network".to_string(),
            "TRX".to_string(),
            false,
            BillKind::Transfer,
            String::new(),
        );
        BillRepo::create(bill, &pool).await.unwrap();

        let page = BillDao::recent_bill(
            "",
            "owner's address",
            "tron'network",
            None,
            0,
            10,
            pool.into_inner(),
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].tx_hash, "recent_bill_hash");
        assert_eq!(page.data[0].address, "recipient's address");
    }

    #[tokio::test]
    async fn bill_repo_get_by_hash_opt_missing_returns_none() {
        let dir = make_temp_dir("wallet_db_bill_repo_missing");
        let ctx = crate::SqliteContext::new(&dir, Some("data.db")).await.unwrap();
        let pool = ctx.into_core_db_pool().unwrap();

        let found = BillRepo::get_by_hash_opt("not_exists", &pool).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn bill_repo_tx_rollback_keeps_bill_absent() {
        let pool = setup_core_pool("wallet_db_bill_repo_rollback").await;

        let mut bill = BillRepo::build_bill(
            "tx_hash_rb".to_string(),
            "from_rb".to_string(),
            "to_rb".to_string(),
            1.0,
            wallet_types::constant::chain_code::TRON.to_string(),
            "TRX".to_string(),
            false,
            BillKind::Transfer,
            "rollback".to_string(),
        );
        bill.status = BillStatus::Pending.to_i8();

        let mut tx = pool.write_ref().begin().await.unwrap();
        BillDao::create(bill, tx.as_mut()).await.unwrap();
        tx.rollback().await.unwrap();

        let found = BillRepo::get_by_hash_opt("tx_hash_rb", &pool).await.unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn bill_repo_build_signed_bill_sets_expected_defaults() {
        let bill =
            BillRepo::build_signed_bill("txh".to_string(), "from".to_string(), "sol".to_string());
        assert_eq!(bill.hash, "txh");
        assert_eq!(bill.from, "from");
        assert_eq!(bill.chain_code, "sol");
        assert_eq!(bill.tx_kind.to_i8(), BillKind::SigningFee.to_i8());
    }

    #[test]
    fn bill_repo_build_deploy_bill_sets_deploy_kind() {
        let bill = BillRepo::build_deploy_bill(
            "txd".to_string(),
            "init".to_string(),
            "tron".to_string(),
            "TRX".to_string(),
        );
        assert_eq!(bill.hash, "txd");
        assert_eq!(bill.from, "init");
        assert_eq!(bill.chain_code, "tron");
        assert_eq!(bill.symbol, "TRX");
        assert_eq!(bill.tx_kind.to_i8(), BillKind::DeployMultiSign.to_i8());
    }
}
