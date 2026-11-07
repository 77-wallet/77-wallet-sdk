use super::DeletionScenario;
use crate::harness::{self, ApiWalletBackendCall};
use wallet_database::{
    entities::api_wallet::ApiWalletType, repositories::api_wallet::wallet::ApiWalletRepo,
};

const BACKGROUND_ADDRESSES: i64 = 500_000;
type BackgroundSignature = (i64, i64, i64, i64, i64);

pub struct BackgroundWallet {
    address: String,
    uid: String,
    signature: BackgroundSignature,
}

impl DeletionScenario {
    pub async fn given_background_addresses(&self) -> BackgroundWallet {
        let uid = harness::next_tag("deletion-background");
        let address = harness::upsert_wallet(
            &self.env.db_dir,
            &self.env.sn,
            &uid,
            ApiWalletType::SubAccount,
            None,
        )
        .await;
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let mut tx = pool.write_ref().begin().await.unwrap();
        // Older background rows precede the target's bulk rows, so this also
        // exercises a large gap in the deletion cursor's global rowid range.
        let inserted = sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x < ?) INSERT INTO api_account (account_id,name,address,pubkey,address_type,wallet_address,uid,derivation_path,derivation_path_index,chain_code,api_wallet_type,status,is_init,is_expand,is_used,created_at) SELECT x,'mixed-scale',printf('mixed-%s-%040d',?,x),'public','',?,?,'path',x,'base',1,x%2,0,0,0,'2026-11-02T00:00:00Z' FROM n")
            .bind(BACKGROUND_ADDRESSES).bind(&uid).bind(&address).bind(&uid)
            .execute(&mut *tx).await.unwrap().rows_affected();
        assert_eq!(inserted, BACKGROUND_ADDRESSES as u64);
        tx.commit().await.unwrap();
        let mut background = BackgroundWallet { address, uid, signature: (0, 0, 0, 0, 0) };
        background.signature = self.background_signature(&background).await;
        assert_eq!(background.signature.0, BACKGROUND_ADDRESSES);
        assert_eq!(background.signature.4, BACKGROUND_ADDRESSES);
        println!("MIXED_SCALE background_before={:?}", background.signature);
        background
    }

    async fn background_signature(&self, background: &BackgroundWallet) -> BackgroundSignature {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        // Aggregate in SQLite: the verifier never materializes 500k accounts.
        sqlx::query_as("SELECT COUNT(*),COALESCE(MIN(id),0),COALESCE(MAX(id),0),COALESCE(SUM(id),0),COALESCE(SUM(CASE WHEN uid=? AND chain_code='base' AND address=printf('mixed-%s-%040d',?,account_id) AND account_id BETWEEN 1 AND 500000 AND status=account_id%2 THEN 1 ELSE 0 END),0) FROM api_account WHERE wallet_address=?")
            .bind(&background.uid).bind(&background.uid).bind(&background.address)
            .fetch_one(pool.read_ref()).await.unwrap()
    }

    pub async fn then_background_unchanged(&self, background: &BackgroundWallet) {
        let signature = self.background_signature(background).await;
        assert_eq!(signature, background.signature, "all background identities must survive");
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let wallet = ApiWalletRepo::find_by_address(&pool, &background.address)
            .await
            .unwrap()
            .expect("background wallet must survive");
        assert_eq!(wallet.uid, background.uid);
        self.env.fake_backend.with_calls(|calls| {
            let call = calls
                .iter()
                .find_map(|call| match call {
                    ApiWalletBackendCall::DeviceDelete(call) => Some(call),
                    _ => None,
                })
                .expect("deletion backend call");
            assert!(call.uid_list.contains(&background.uid), "retain background device binding");
        });
        println!("MIXED_SCALE background_after={signature:?}");
    }
}
