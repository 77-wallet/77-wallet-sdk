use crate::harness::{self, TestEnv, WalletPair};
use wallet_database::{
    entities::{
        api_account::CreateApiAccountVo, api_wallet::ApiWalletType,
        api_withdraw_strategy::ApiWithdrawStrategyEntity,
        api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigEntity,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, withdraw_strategy::ApiWithdrawStrategyRepo,
    },
};

pub struct Scenario {
    env: &'static TestEnv,
    pair: WalletPair,
    permission: String,
    target: String,
}
impl Scenario {
    pub async fn given() -> Self {
        let env = harness::ensure_env().await;
        let pair = harness::prepare_wallet_pair(env).await;
        let pool = harness::open_api_wallet_pool(&env.db_dir).await;
        let permission = harness::next_tag("delete-account-permission");
        let base = harness::next_unique_id() as u64 * 10;
        let tron = |i: u64| {
            wallet_utils::address::hex_to_bs58_addr(&format!("41{:040x}", base + i)).unwrap()
        };
        let target = tron(2);
        let mut accounts = vec![];
        for i in 0..3 {
            let index = wallet_utils::address::AccountIndexMap::from_input_index(i).unwrap();
            for chain in ["tron", "base"] {
                let address = if chain == "tron" {
                    tron(i as u64)
                } else {
                    format!("0x{:040x}", base + i as u64)
                };
                accounts.push(CreateApiAccountVo::new(
                    index.account_id,
                    &address,
                    "fixture-public-key",
                    &pair.withdrawal_address,
                    &pair.withdrawal_uid,
                    "fixture-path",
                    i,
                    chain,
                    "account-delete",
                    ApiWalletType::Withdrawal,
                ));
            }
        }
        ApiAccountRepo::upsert_account_multi(&pool, accounts).await.unwrap();
        // An unrelated wallet owns the same logical index; deletion must remain wallet scoped.
        let other_index = wallet_utils::address::AccountIndexMap::from_input_index(2).unwrap();
        ApiAccountRepo::upsert_account_multi(
            &pool,
            vec![CreateApiAccountVo::new(
                other_index.account_id,
                &tron(3),
                "fixture-public-key",
                &pair.recharge_address,
                &pair.recharge_uid,
                "fixture-path",
                2,
                "tron",
                "other-wallet",
                ApiWalletType::SubAccount,
            )],
        )
        .await
        .unwrap();
        ApiWithdrawStrategyRepo::replace_with_chain_configs(
            &pool,
            ApiWithdrawStrategyEntity {
                id: 0,
                uid: pair.withdrawal_uid.clone(),
                threshold: 1,
                created_at: Default::default(),
                updated_at: None,
            },
            vec![ApiWithdrawStrategyChainConfigEntity {
                id: 0,
                strategy_id: 0,
                chain_code: "tron".into(),
                chain_address_type: None,
                normal_idx: Some(0),
                normal_address: tron(0),
                risk_idx: Some(1),
                risk_address: tron(1),
                created_at: Default::default(),
                updated_at: None,
            }],
        )
        .await
        .unwrap();
        let core = harness::open_core_pool(&env.db_dir).await;
        sqlx::query("INSERT INTO permission(id,name,grantor_addr,types,active_id,threshold,member,chain_code,created_at) VALUES(?, 'fixture', ?, 'active', 2, 1, 2, 'tron', datetime('now'))")
            .bind(&permission).bind(&permission).execute(core.write_ref()).await.unwrap();
        for address in [&target, &tron(0)] {
            sqlx::query("INSERT INTO permission_user(grantor_addr,address,permission_id,is_self,weight,created_at) VALUES(?,?,?,1,1,datetime('now'))")
                .bind(&permission).bind(address).bind(&permission).execute(core.write_ref()).await.unwrap();
        }
        Self { env, pair, permission, target }
    }
    pub async fn when_delete(
        &self,
        input: i32,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let index = wallet_utils::address::AccountIndexMap::from_input_index(input).unwrap();
        self.env
            .manager
            .physical_delete_api_account(
                &self.pair.withdrawal_address,
                index.account_id,
                harness::SMOKE_WALLET_PASSWORD,
            )
            .await
    }
    async fn count(&self, input: i32) -> i64 {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let index = wallet_utils::address::AccountIndexMap::from_input_index(input).unwrap();
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_account WHERE wallet_address=? AND account_id=?",
        )
        .bind(&self.pair.withdrawal_address)
        .bind(index.account_id)
        .fetch_one(pool.read_ref())
        .await
        .unwrap()
    }
    pub async fn then_target_absent(&self) {
        assert_eq!(self.count(2).await, 0);
    }
    pub async fn then_other_accounts_unchanged(&self) {
        for i in 0..2 {
            assert_eq!(self.count(i).await, 2);
        }
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let other: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE wallet_address=?")
                .bind(&self.pair.recharge_address)
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        assert_eq!(other, 1, "other wallet must survive");
    }
    pub async fn then_all_accounts_present(&self) {
        self.then_other_accounts_unchanged().await;
        assert_eq!(self.count(2).await, 2);
    }
    pub async fn given_no_permissions(&self) {
        let pool = harness::open_core_pool(&self.env.db_dir).await;
        sqlx::query("DELETE FROM permission_user WHERE permission_id=?")
            .bind(&self.permission)
            .execute(pool.write_ref())
            .await
            .unwrap();
        sqlx::query("DELETE FROM permission WHERE id=?")
            .bind(&self.permission)
            .execute(pool.write_ref())
            .await
            .unwrap();
    }
    async fn membership(&self) -> i64 {
        let pool = harness::open_core_pool(&self.env.db_dir).await;
        sqlx::query_scalar(
            "SELECT is_self FROM permission_user WHERE permission_id=? AND address=?",
        )
        .bind(&self.permission)
        .bind(&self.target)
        .fetch_one(pool.read_ref())
        .await
        .unwrap()
    }
    pub async fn then_permission_membership_updated(&self) {
        assert_eq!(self.membership().await, 0);
    }
    pub async fn then_permission_membership_unchanged(&self) {
        assert_eq!(self.membership().await, 1);
    }
}
