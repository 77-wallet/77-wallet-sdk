use rand::RngCore;
use std::{
    collections::HashSet,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use wallet_api::{
    domain::chain::adapter::MultisigAdapter, request::wallet::CreateWalletReq,
    response_vo::TransferParams,
};
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_database::{
    dao::assets::CreateAssetsVo,
    entities::{
        api_chain::NodeBindType,
        asset_token_key::AssetTokenKey,
        assets::{AssetsEntity, AssetsId},
        chain::{ChainCreateVo, ChainWithNode},
        coin::CoinData,
        multisig_account::NewMultisigAccountEntity,
        multisig_member::MemberVo,
        multisig_queue::MultisigQueueStatus,
        multisig_signatures::MultisigSignatureStatus,
        node::NodeCreateVo,
    },
    repositories::{
        account::AccountRepo, assets::AssetsRepo, chain::ChainRepo, coin::CoinRepo,
        multisig_account::MultisigAccountRepo, multisig_member::MultisigMemberRepo,
        multisig_queue::MultisigQueueRepo, node::NodeRepo,
    },
};
use wallet_transport_backend::response_vo::api_wallet::wallet::UidStatus;
use wallet_types::chain::chain::ChainCode;

use crate::harness::{SMOKE_WALLET_PASSWORD, TestEnv, ensure_env, next_tag, open_core_pool};

pub(super) struct EvmDeployScenario {
    env: &'static TestEnv,
    account_id: String,
    chain_code: String,
    rpc_url: String,
}

pub(super) const TEST_EVM_TOKEN: &str = "0x4444444444444444444444444444444444444444";

impl EvmDeployScenario {
    fn native_symbol(&self) -> &'static str {
        match self.chain_code.as_str() {
            "avalanche" => "AVAX",
            "arbitrum" | "base" | "scroll" => "ETH",
            _ => "POL",
        }
    }

    pub(super) async fn given(chain_code: &str, chain_name: &str, rpc_url: &str) -> Self {
        let env = ensure_env().await;
        let pool = open_core_pool(&env.db_dir).await;
        let node_id = next_tag("evm-safe-node");
        ChainRepo::add(
            &pool,
            ChainCreateVo::new(
                chain_name,
                chain_code,
                &["evm".to_string()],
                NodeBindType::ManualUser,
                "SAFE",
            ),
        )
        .await
        .expect("seed EVM chain");
        NodeRepo::upsert(
            &pool,
            NodeCreateVo::new(&node_id, "Safe fake RPC", chain_code, rpc_url, None),
        )
        .await
        .expect("seed EVM node");
        ChainRepo::set_chain_node_with_type(&pool, chain_code, &node_id, NodeBindType::ManualUser)
            .await
            .expect("select EVM node");

        let account_id = next_tag("evm-safe-account");
        let account = NewMultisigAccountEntity::new(
            Some(account_id.clone()),
            format!("{chain_name} preflight test"),
            "0x1111111111111111111111111111111111111111".to_string(),
            "".to_string(),
            chain_code.to_string(),
            1,
            "".to_string(),
            vec![MemberVo::new(
                "owner".to_string(),
                "0x1111111111111111111111111111111111111111".to_string(),
            )],
            &HashSet::new(),
        );
        MultisigAccountRepo::create_with_member(&pool, &account)
            .await
            .expect("seed EVM multisig account");
        Self { env, account_id, chain_code: chain_code.to_string(), rpc_url: rpc_url.to_string() }
    }

    pub(super) async fn given_pending_deployment(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [
                ("status".to_string(), "5".to_string()),
                ("deploy_hash".to_string(), format!("0x{}", "11".repeat(32))),
            ]
            .into_iter()
            .collect(),
        )
        .await
        .expect("mark deployment pending");
    }

    pub(super) async fn given_paid_failed_deployment_with_address(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [
                ("pay_status".to_string(), "1".to_string()),
                ("status".to_string(), "4".to_string()),
                ("address".to_string(), "0x2222222222222222222222222222222222222222".to_string()),
                ("salt".to_string(), "12345".to_string()),
            ]
            .into_iter()
            .collect(),
        )
        .await
        .expect("seed failed deployment with stored Safe address");
    }

    pub(super) async fn given_signable_paid_deployment(&self) {
        let phrase = self.env.manager.generate_phrase(1, 12).expect("generate test phrase");
        self.env.fake_backend.enqueue_keys_uid_status(UidStatus::NotFound);
        let wallet = self
            .env
            .manager
            .create_wallet(CreateWalletReq::new(
                1,
                &phrase.phrases.join(" "),
                &next_tag("safe-test-salt"),
                &next_tag("safe-test-wallet"),
                "Safe test account",
                false,
                SMOKE_WALLET_PASSWORD,
                None,
                None,
            ))
            .await
            .expect("create ephemeral Core wallet");
        let pool = open_core_pool(&self.env.db_dir).await;
        let initiator = AccountRepo::list(pool.clone())
            .await
            .expect("list Core accounts")
            .into_iter()
            .find(|account| {
                account.wallet_address == wallet.address && account.chain_code == self.chain_code
            })
            .expect("derived EVM account");
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [
                ("pay_status".to_string(), "1".to_string()),
                ("status".to_string(), "4".to_string()),
                ("address".to_string(), "0x2222222222222222222222222222222222222222".to_string()),
                ("salt".to_string(), "12345".to_string()),
                ("initiator_addr".to_string(), initiator.address),
            ]
            .into_iter()
            .collect(),
        )
        .await
        .expect("seed paid Safe deployment");
        let now = sqlx::types::chrono::Utc::now();
        let native_symbol = self.native_symbol();
        CoinRepo::upsert_multi_coin(
            &pool,
            vec![CoinData::new(
                Some(self.chain_code.clone()),
                native_symbol,
                &self.chain_code,
                AssetTokenKey::Native,
                None,
                None,
                18,
                0,
                0,
                1,
                false,
                now,
                now,
            )],
        )
        .await
        .expect("seed EVM native coin");
    }

    pub(super) async fn then_native_coin_symbol(&self, expected: &str) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let coin = CoinRepo::main_coin(&self.chain_code, &pool).await.expect("load native coin");
        assert_eq!(coin.symbol, expected);
    }

    pub(super) async fn given_signable_onchain_native_safe(&self) {
        self.given_signable_paid_deployment().await;
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load Safe")
            .expect("Safe exists");
        let mut address_bytes = [0u8; 20];
        rand::rngs::OsRng.fill_bytes(&mut address_bytes);
        let safe_address = format!("0x{}", hex::encode(address_bytes));
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [
                ("status".to_string(), "3".to_string()),
                ("address".to_string(), safe_address.clone()),
            ]
            .into_iter()
            .collect(),
        )
        .await
        .expect("mark Safe on chain");
        sqlx::query("UPDATE multisig_member SET address = ?, is_self = 1, confirmed = 1 WHERE account_id = ?")
            .bind(&account.initiator_addr)
            .bind(&self.account_id)
            .execute(pool.write_ref())
            .await.expect("link local Safe signer");
        let native_symbol = self.native_symbol();
        AssetsRepo::upsert_assets(
            &pool,
            CreateAssetsVo::new(
                AssetsId::new(&safe_address, &self.chain_code, AssetTokenKey::Native),
                native_symbol,
                18,
                None,
                1,
            ),
        )
        .await
        .expect("seed native Safe asset");
    }

    pub(super) async fn given_two_of_two_safe(&self, second_signer_local: bool) {
        let pool = open_core_pool(&self.env.db_dir).await;
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [("threshold".to_string(), "2".to_string())].into_iter().collect(),
        )
        .await
        .expect("set 2-of-2 threshold");
        let second_address = if second_signer_local {
            let phrase = self.env.manager.generate_phrase(1, 12).expect("generate test phrase");
            self.env.fake_backend.enqueue_keys_uid_status(UidStatus::NotFound);
            let wallet = self
                .env
                .manager
                .create_wallet(CreateWalletReq::new(
                    1,
                    &phrase.phrases.join(" "),
                    &next_tag("safe-second-salt"),
                    &next_tag("safe-second-wallet"),
                    "Second Safe signer",
                    false,
                    SMOKE_WALLET_PASSWORD,
                    None,
                    None,
                ))
                .await
                .expect("create second ephemeral Core wallet");
            AccountRepo::list(pool.clone())
                .await
                .expect("list Core accounts")
                .into_iter()
                .find(|account| {
                    account.wallet_address == wallet.address
                        && account.chain_code == self.chain_code
                })
                .expect("derived second EVM account")
                .address
        } else {
            "0x9999999999999999999999999999999999999999".to_string()
        };
        sqlx::query("INSERT INTO multisig_member (account_id, address, name, confirmed, is_self, created_at) VALUES (?, ?, 'second owner', 1, ?, ?)")
            .bind(&self.account_id)
            .bind(&second_address)
            .bind(i32::from(second_signer_local))
            .bind(sqlx::types::chrono::Utc::now())
            .execute(pool.write_ref())
            .await
            .expect("seed second Safe owner");
    }

    pub(super) async fn then_native_queue_has_signatures(
        &self,
        count: usize,
        status: MultisigQueueStatus,
    ) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queue")
            .expect("queue exists");
        assert_eq!(queue.status, status.to_i8());
        assert!(queue.tx_hash.is_empty());
        let signatures = MultisigQueueRepo::get_signed_list(&pool, &queue.id)
            .await
            .expect("load Safe signatures");
        assert_eq!(signatures.0.len(), count);
        assert!(
            signatures
                .0
                .iter()
                .all(|sign| sign.status == MultisigSignatureStatus::Approved.to_i8()
                    && sign.signature.len() == 130)
        );
    }

    pub(super) async fn given_signable_onchain_token_safe(&self) {
        self.given_signable_onchain_native_safe().await;
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load Safe")
            .expect("Safe exists");
        let token = AssetTokenKey::Contract(TEST_EVM_TOKEN.to_string());
        let now = sqlx::types::chrono::Utc::now();
        CoinRepo::upsert_multi_coin(
            &pool,
            vec![CoinData::new(
                Some("Test token".to_string()),
                "TST",
                &self.chain_code,
                token.clone(),
                None,
                None,
                6,
                0,
                0,
                1,
                false,
                now,
                now,
            )],
        )
        .await
        .expect("seed EVM token coin");
        AssetsRepo::upsert_assets(
            &pool,
            CreateAssetsVo::new(
                AssetsId::new(&account.address, &self.chain_code, token),
                "TST",
                6,
                None,
                1,
            ),
        )
        .await
        .expect("seed Safe token asset");
    }

    pub(super) async fn when_create_native_queue(&self) -> Result<String, String> {
        self.when_create_queue(None).await
    }

    pub(super) async fn when_create_token_queue(&self) -> Result<String, String> {
        self.when_create_queue(Some(TEST_EVM_TOKEN)).await
    }

    async fn when_create_queue(&self, token: Option<&str>) -> Result<String, String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .map_err(|error| error.to_string())?
            .expect("Safe exists");
        let request = TransferParams {
            from: account.address,
            to: "0x3333333333333333333333333333333333333333".to_string(),
            value: "0.1".to_string(),
            expiration: None,
            chain_code: self.chain_code.clone(),
            symbol: if token.is_some() { "TST" } else { self.native_symbol() }.to_string(),
            token_address: token.map(str::to_string),
            notes: None,
            spend_all: false,
            signer: None,
        };
        self.env
            .manager
            .create_multisig_queue(request, SMOKE_WALLET_PASSWORD.to_string())
            .await
            .map_err(|error| error.to_string())
    }

    pub(super) async fn then_token_queue_is_persisted(&self, expected_hash: &str) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queue")
            .expect("queue exists");
        assert_eq!(queue.msg_hash, expected_hash);
        assert_eq!(queue.token_addr, AssetTokenKey::Contract(TEST_EVM_TOKEN.to_string()));
        assert_eq!(queue.status, MultisigQueueStatus::PendingExecution.to_i8());
        let payload: wallet_chain_interact::eth::operations::MultisigPayloadOpt =
            queue.raw_data.parse().expect("decode token proposal");
        assert_eq!(payload.sign_message, expected_hash);
        assert!(payload.input_data.to_lowercase().contains(&TEST_EVM_TOKEN[2..]));
        let signatures = MultisigQueueRepo::get_signed_list(&pool, &queue.id)
            .await
            .expect("load token signatures");
        assert_eq!(signatures.0.len(), 1);
        assert_eq!(signatures.0[0].status, MultisigSignatureStatus::Approved.to_i8());
        assert_eq!(signatures.0[0].signature.len(), 130);
    }

    pub(super) async fn then_native_queue_is_persisted(&self, expected_hash: &str) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load Safe")
            .expect("Safe exists");
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queues")
            .expect("queue exists");
        assert_eq!(queue.msg_hash, expected_hash);
        assert!(!queue.raw_data.is_empty());
        assert_eq!(queue.from_addr, account.address);
        assert_eq!(queue.symbol, self.native_symbol());
        assert_eq!(queue.status, MultisigQueueStatus::PendingExecution.to_i8());
        assert!(queue.tx_hash.is_empty());
        let signatures =
            MultisigQueueRepo::get_signed_list(&pool, &queue.id).await.expect("load signatures");
        assert_eq!(signatures.0.len(), 1);
        assert_eq!(signatures.0[0].status, MultisigSignatureStatus::Approved.to_i8());
        assert_eq!(signatures.0[0].signature.len(), 130);
    }

    pub(super) async fn then_no_native_queue_exists(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        assert!(
            MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
                .await
                .expect("load Safe queues")
                .is_none()
        );
    }

    pub(super) async fn when_execute_native_queue(&self) -> Result<String, String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .map_err(|error| error.to_string())?
            .expect("queue exists");
        self.env.manager.exec_transaction(
            queue.id,
            SMOKE_WALLET_PASSWORD.to_string(),
            Some(r#"{"gasLimit":262975,"baseFee":"3329262291","priorityFee":"0","maxFeePerGas":"3995114749"}"#.to_string()),
            None,
        ).await.map_err(|error| error.to_string())
    }

    pub(super) async fn when_execute_token_queue(&self) -> Result<String, String> {
        self.when_execute_native_queue().await
    }

    pub(super) async fn then_token_queue_execution_persisted(&self, hash: &str) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queue")
            .expect("queue exists");
        assert_eq!(queue.status, MultisigQueueStatus::InConfirmation.to_i8());
        assert_eq!(queue.tx_hash, hash);
        let (token, status): (String, i8) =
            sqlx::query_as("SELECT token, status FROM bill WHERE queue_id = ? AND hash = ?")
                .bind(&queue.id)
                .bind(hash)
                .fetch_one(pool.read_ref())
                .await
                .expect("load token bill");
        assert_eq!(
            AssetTokenKey::from_db_value(&token),
            AssetTokenKey::Contract(TEST_EVM_TOKEN.to_string())
        );
        assert_eq!(status, 1);
    }

    pub(super) async fn then_native_queue_execution_persisted(&self, hash: &str) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queue")
            .expect("queue exists");
        assert_eq!(queue.status, MultisigQueueStatus::InConfirmation.to_i8());
        assert_eq!(queue.tx_hash, hash);
        assert_eq!(queue.symbol, self.native_symbol());
        let bill_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM bill WHERE queue_id = ? AND hash = ?")
                .bind(&queue.id)
                .bind(hash)
                .fetch_one(pool.read_ref())
                .await
                .expect("count execution bills");
        assert_eq!(bill_count, 1);
        let token: String =
            sqlx::query_scalar("SELECT token FROM bill WHERE queue_id = ? AND hash = ?")
                .bind(&queue.id)
                .bind(hash)
                .fetch_one(pool.read_ref())
                .await
                .expect("load native bill");
        assert_eq!(AssetTokenKey::from_db_value(&token), AssetTokenKey::Native);
    }

    pub(super) async fn then_native_queue_remains_ready(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queue")
            .expect("queue exists");
        assert_eq!(queue.status, MultisigQueueStatus::PendingExecution.to_i8());
        assert!(queue.tx_hash.is_empty());
        let bill_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bill WHERE queue_id = ?")
            .bind(&queue.id)
            .fetch_one(pool.read_ref())
            .await
            .expect("count execution bills");
        assert_eq!(bill_count, 0);
    }

    pub(super) async fn when_sync_queue_execution(&self) -> Result<usize, String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .map_err(|error| error.to_string())?
            .expect("queue exists");
        let bill_id: i64 = sqlx::query_scalar("SELECT id FROM bill WHERE queue_id = ?")
            .bind(&queue.id)
            .fetch_one(pool.read_ref())
            .await
            .map_err(|error| error.to_string())?;
        let result = self
            .env
            .manager
            .query_tx_result(vec![bill_id.to_string()])
            .await
            .map_err(|error| error.to_string())?;
        Ok(result.len())
    }

    pub(super) async fn then_queue_execution_status(&self, bill_status: i8, queue_status: i8) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let queue = MultisigQueueRepo::list_by_account_ids(&pool, &[self.account_id.clone()])
            .await
            .expect("load Safe queue")
            .expect("queue exists");
        assert_eq!(queue.status, queue_status);
        let status: i8 = sqlx::query_scalar("SELECT status FROM bill WHERE queue_id = ?")
            .bind(&queue.id)
            .fetch_one(pool.read_ref())
            .await
            .expect("load Safe bill status");
        assert_eq!(status, bill_status);
    }

    pub(super) async fn then_token_asset_balance(&self, expected: f64) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load Safe")
            .expect("Safe exists");
        let asset = AssetsRepo::assets_by_id(
            &pool,
            &AssetsId::new(
                &account.address,
                &self.chain_code,
                AssetTokenKey::Contract(TEST_EVM_TOKEN.to_string()),
            ),
        )
        .await
        .expect("load token asset")
        .expect("token asset exists");
        assert_eq!(asset.balance.parse::<f64>().expect("numeric balance"), expected);
    }

    pub(super) async fn then_native_asset_balance(&self, expected: f64) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load Safe")
            .expect("Safe exists");
        let asset = AssetsRepo::assets_by_id(
            &pool,
            &AssetsId::new(&account.address, &self.chain_code, AssetTokenKey::Native),
        )
        .await
        .expect("load native asset")
        .expect("native asset exists");
        assert_eq!(asset.balance.parse::<f64>().expect("numeric balance"), expected);
    }

    pub(super) async fn when_service_deploys(&self) -> Result<(), String> {
        self.env
            .manager
            .deploy_multisig_account(
                self.account_id.clone(),
                Some(
                    r#"{"gasLimit":262975,"baseFee":"3329262291","priorityFee":"0","maxFeePerGas":"3995114749"}"#
                        .to_string(),
                ),
                None,
                SMOKE_WALLET_PASSWORD.to_string(),
            )
            .await
            .map_err(|error| error.to_string())
    }

    pub(super) async fn then_service_deployment_pending(&self, hash: &str) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load account")
            .expect("account exists");
        assert_eq!(account.status, 5);
        assert_eq!(account.deploy_hash, hash);
        assert_eq!(account.address, "0x2222222222222222222222222222222222222222");
        assert_eq!(account.salt, "12345");
    }

    pub(super) async fn given_paid_pending_deployment_with_hash(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [
                ("pay_status".to_string(), "1".to_string()),
                ("status".to_string(), "5".to_string()),
                ("address".to_string(), "0x2222222222222222222222222222222222222222".to_string()),
                ("salt".to_string(), "12345".to_string()),
                ("deploy_hash".to_string(), format!("0x{}", "11".repeat(32))),
            ]
            .into_iter()
            .collect(),
        )
        .await
        .expect("seed pending deployment with hash");
    }

    pub(super) async fn when_retry_pending_deployment(&self) -> Result<(), String> {
        self.env
            .manager
            .deploy_multisig_account(self.account_id.clone(), None, None, "unused".to_string())
            .await
            .map_err(|error| error.to_string())
    }

    pub(super) async fn then_pending_deployment_is_unchanged(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load account")
            .expect("account exists");
        assert_eq!(account.pay_status, 1);
        assert_eq!(account.status, 5);
        assert_eq!(account.address, "0x2222222222222222222222222222222222222222");
        assert_eq!(account.salt, "12345");
        assert_eq!(account.deploy_hash, format!("0x{}", "11".repeat(32)));
    }

    pub(super) async fn then_failed_deployment_with_hash_is_unchanged(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load account")
            .expect("account exists");
        assert_eq!(account.status, 4);
        assert_eq!(account.address, "0x2222222222222222222222222222222222222222");
        assert_eq!(account.salt, "12345");
        assert_eq!(account.deploy_hash, format!("0x{}", "11".repeat(32)));
    }

    pub(super) async fn when_retry_deployment_without_signer(&self) -> String {
        self.env
            .manager
            .deploy_multisig_account(self.account_id.clone(), None, None, "unused".to_string())
            .await
            .expect_err("missing signer must reject deployment")
            .to_string()
    }

    pub(super) async fn when_broadcast_deployment_with_test_signer(
        &self,
    ) -> Result<(String, String), String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let mut account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .map_err(|error| error.to_string())?
            .expect("account exists");
        let members = MultisigMemberRepo::list_by_account_id(&pool, &self.account_id)
            .await
            .map_err(|error| error.to_string())?;
        let node = ChainWithNode {
            chain_code: self.chain_code.clone(),
            rpc_url: self.rpc_url.clone(),
            network: "mainnet".to_string(),
            ..Default::default()
        };
        let chain =
            ChainCode::try_from(self.chain_code.as_str()).map_err(|error| error.to_string())?;
        let adapter = MultisigAdapter::new(chain, node, None).map_err(|error| error.to_string())?;
        let mut signing_key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut signing_key);
        let key_hex = hex::encode(signing_key);
        let signer: alloy::signers::local::PrivateKeySigner =
            key_hex.parse().map_err(|error| format!("test signer: {error}"))?;
        account.initiator_addr = signer.address().to_string();
        let key = ChainPrivateKey::from(key_hex);
        adapter
            .deploy_multisig_account(
                &account,
                &members,
                Some(
                    r#"{"gasLimit":262975,"baseFee":"3329262291","priorityFee":"0","maxFeePerGas":"3995114749"}"#
                        .to_string(),
                ),
                key,
            )
            .await
            .map_err(|error| error.to_string())
    }

    pub(super) async fn when_build_native_proposal(
        &self,
        value: &str,
    ) -> Result<(String, String), String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .map_err(|error| error.to_string())?
            .expect("account exists");
        let node = ChainWithNode {
            chain_code: self.chain_code.clone(),
            rpc_url: self.rpc_url.clone(),
            network: "mainnet".to_string(),
            ..Default::default()
        };
        let chain =
            ChainCode::try_from(self.chain_code.as_str()).map_err(|error| error.to_string())?;
        let adapter = MultisigAdapter::new(chain, node, None).map_err(|error| error.to_string())?;
        let transfer = TransferParams {
            from: account.address.clone(),
            to: "0x3333333333333333333333333333333333333333".to_string(),
            value: value.to_string(),
            expiration: None,
            chain_code: self.chain_code.clone(),
            symbol: self.native_symbol().to_string(),
            token_address: None,
            notes: None,
            spend_all: false,
            signer: None,
        };
        let assets = AssetsEntity {
            decimals: 18,
            token_address: AssetTokenKey::Native,
            ..Default::default()
        };
        let result = adapter
            .build_multisig_with_account(
                &transfer,
                &account,
                &assets,
                ChainPrivateKey::from(String::new()),
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok((result.tx_hash, result.raw_data))
    }

    pub(super) async fn when_sign_native_proposal(
        &self,
        raw_data: &str,
    ) -> Result<(String, Vec<u8>), String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .map_err(|error| error.to_string())?
            .expect("account exists");
        let node = ChainWithNode {
            chain_code: self.chain_code.clone(),
            rpc_url: self.rpc_url.clone(),
            network: "mainnet".to_string(),
            ..Default::default()
        };
        let chain =
            ChainCode::try_from(self.chain_code.as_str()).map_err(|error| error.to_string())?;
        let adapter = MultisigAdapter::new(chain, node, None).map_err(|error| error.to_string())?;
        let secp = secp256k1::Secp256k1::new();
        let secret = secp256k1::SecretKey::new(&mut rand::rngs::OsRng);
        let public = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize_uncompressed();
        let result = adapter
            .sign_multisig_tx(
                &account,
                &account.initiator_addr,
                ChainPrivateKey::from(hex::encode(secret.secret_bytes())),
                raw_data,
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok((result.signature, public.to_vec()))
    }

    pub(super) async fn then_stored_address_is_unchanged(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load account")
            .expect("account exists");
        assert_eq!(account.address, "0x2222222222222222222222222222222222222222");
        assert_eq!(account.salt, "12345");
        assert_eq!(account.status, 4);
        assert!(account.deploy_hash.is_empty());
    }

    pub(super) async fn when_sync_deployment(&self) -> i8 {
        self.env
            .manager
            .multisig_account_lists(true, Some(self.chain_code.clone()), 1, 1_000)
            .await
            .expect("list accounts");
        let pool = open_core_pool(&self.env.db_dir).await;
        MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load account")
            .expect("account exists")
            .status
    }

    pub(super) async fn then_stop_syncing(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        MultisigAccountRepo::update_by_id(
            &pool,
            &self.account_id,
            [("status".to_string(), "4".to_string())].into_iter().collect(),
        )
        .await
        .expect("clear pending test account");
    }

    pub(super) async fn when_derive_address(&self) -> Result<(String, String), String> {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .map_err(|error| format!("{error:?}"))?
            .expect("account exists");
        let members = MultisigMemberRepo::list_by_account_id(&pool, &self.account_id)
            .await
            .map_err(|error| format!("{error:?}"))?;
        let node = ChainWithNode {
            chain_code: self.chain_code.clone(),
            rpc_url: self.rpc_url.clone(),
            network: "mainnet".to_string(),
            ..Default::default()
        };
        let chain_code =
            ChainCode::try_from(self.chain_code.as_str()).map_err(|error| format!("{error:?}"))?;
        let adapter =
            MultisigAdapter::new(chain_code, node, None).map_err(|error| format!("{error:?}"))?;
        let address = adapter
            .multisig_address(&account, &members)
            .await
            .map_err(|error| format!("{error:?}"))?;
        Ok((address.multisig_address, address.salt))
    }

    pub(super) async fn when_deploy_without_payer(&self) -> Result<(), String> {
        self.env
            .manager
            .deploy_multisig_account(self.account_id.clone(), None, None, "unused".to_string())
            .await
            .map_err(|error| format!("{error:?}"))
    }

    pub(super) async fn then_account_unchanged(&self) {
        let pool = open_core_pool(&self.env.db_dir).await;
        let account = MultisigAccountRepo::find_by_id(&pool, &self.account_id)
            .await
            .expect("load account")
            .expect("account exists");
        assert_eq!(account.pay_status, 0);
        assert!(account.fee_hash.is_empty());
        assert!(account.deploy_hash.is_empty());
        assert!(account.salt.is_empty());
    }
}

pub(super) struct FakeEvmRpc {
    pub(super) url: String,
    worker: thread::JoinHandle<Vec<String>>,
}

impl FakeEvmRpc {
    pub(super) fn new(responses: &[(&'static str, &'static str)]) -> Self {
        Self::new_json(
            responses
                .iter()
                .map(|(method, result)| (*method, serde_json::Value::String((*result).to_string())))
                .collect(),
        )
    }

    pub(super) fn new_json(responses: Vec<(&'static str, serde_json::Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake EVM RPC");
        listener.set_nonblocking(true).expect("nonblocking fake RPC");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let mut methods = Vec::new();
            for (expected_method, result) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "RPC request was not received");
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("fake RPC accept failed: {error}"),
                    }
                };
                socket.set_nonblocking(false).expect("blocking fake RPC socket");
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("fake RPC read timeout");
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let (body_start, body_len) = loop {
                    let n = socket.read(&mut buffer).expect("read RPC request");
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let len = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, len);
                    }
                };
                while bytes.len() < body_start + body_len {
                    let n = socket.read(&mut buffer).expect("read RPC body");
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let request: serde_json::Value =
                    serde_json::from_slice(&bytes[body_start..body_start + body_len]).unwrap();
                let method = request["method"].as_str().unwrap();
                assert_eq!(method, expected_method);
                if method == "eth_sendRawTransaction" {
                    let raw = request["params"][0].as_str().expect("signed raw transaction");
                    assert!(raw.starts_with("0x02"), "expected signed EIP-1559 transaction");
                    assert!(raw.len() > 100, "signed transaction must contain a payload");
                }
                methods.push(method.to_string());
                let response = if result == "__rpc_rejected__" {
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": request["id"],
                        "error": { "code": -32000, "message": "transaction rejected by fake RPC" }
                    })
                } else {
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": request["id"], "result": result
                    })
                }
                .to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
            methods
        });
        Self { url, worker }
    }

    pub(super) fn finish(self) -> Vec<String> {
        self.worker.join().expect("fake RPC worker")
    }
}
