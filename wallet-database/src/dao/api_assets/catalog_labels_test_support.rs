use crate::{
    ApiWalletDbPool, repositories::test_helper::setup_api_wallet_pool_before_label_removal,
};

pub(super) async fn fixture(prefix: &str) -> ApiWalletDbPool {
    let pool = setup_api_wallet_pool_before_label_removal(prefix).await;
    sqlx::raw_sql("INSERT INTO api_chain(name,chain_code,protocols,main_symbol,status,created_at)
        VALUES('Ethereum','eth','[]','ETH',1,CURRENT_TIMESTAMP);
        INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,protocol,status,is_default,created_at)
        VALUES('Live Name','LIVE','eth','0xabc','2',18,'catalog-protocol',1,1,CURRENT_TIMESTAMP);
        INSERT INTO api_account(account_id,name,address,address_type,wallet_address,chain_code,uid,api_wallet_type,status,is_init,created_at)
        VALUES(3,'fixture','owner','synthetic','wallet','eth','synthetic',1,1,1,CURRENT_TIMESTAMP);
        INSERT INTO api_assets(name,symbol,decimals,address,token_address,protocol,chain_code,status,balance,created_at,updated_at)
        VALUES('Cached Name','OLD',6,'owner','0xabc','asset-protocol','eth',1,'4','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z');")
        .execute(pool.write_ref()).await.unwrap();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    pool
}

pub(super) async fn stored(pool: &ApiWalletDbPool) -> (String, i64, i64, String, String) {
    sqlx::query_as("SELECT balance,status,decimals,created_at,updated_at FROM api_assets WHERE address='owner' AND token_address='0xabc'")
        .fetch_one(pool.read_ref()).await.unwrap()
}
