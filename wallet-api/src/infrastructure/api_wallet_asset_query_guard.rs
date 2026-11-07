use once_cell::sync::Lazy;
use tokio::time::Duration;

use crate::{
    error::service::ServiceError,
    infrastructure::protected_query::ProtectedQuery,
    response_vo::standard_wallet::assets::{CoinAssets, GetAccountAssetsRes},
};

static ACCOUNT_ASSETS_QUERY: Lazy<ProtectedQuery<GetAccountAssetsRes>> =
    Lazy::new(ProtectedQuery::default);
static ASSET_DETAIL_QUERY: Lazy<ProtectedQuery<CoinAssets>> = Lazy::new(ProtectedQuery::default);

pub(crate) fn account_assets_key(
    wallet_address: &str,
    account_id: u32,
    chain_code: Option<&str>,
) -> String {
    format!(
        "api_wallet_account_assets:wallet={wallet_address};account_id={account_id};chain_code={}",
        chain_code.unwrap_or("none")
    )
}

pub(crate) fn asset_detail_key(
    address: &str,
    account_id: Option<u32>,
    chain_code: &str,
    token_address: Option<&str>,
) -> String {
    let token_key =
        wallet_database::entities::asset_token_key::AssetTokenKey::from_raw(token_address)
            .normalized_for_chain(chain_code);
    let account_part = account_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string());
    format!(
        "api_wallet_asset_detail:address={address};account_id={account_part};chain_code={chain_code};token={}",
        token_key.as_db_str()
    )
}

pub(crate) async fn load_account_assets<F, Fut>(
    key: &str,
    fresh_ttl: Duration,
    query_fn: F,
) -> Result<GetAccountAssetsRes, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<GetAccountAssetsRes, ServiceError>>,
{
    ACCOUNT_ASSETS_QUERY.load(key, fresh_ttl, query_fn).await
}

pub(crate) async fn load_asset_detail<F, Fut>(
    key: &str,
    fresh_ttl: Duration,
    query_fn: F,
) -> Result<CoinAssets, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<CoinAssets, ServiceError>>,
{
    ASSET_DETAIL_QUERY.load(key, fresh_ttl, query_fn).await
}

pub(crate) fn mark_account_assets_stale(wallet_address: &str, fresh_ttl: Duration) -> usize {
    ACCOUNT_ASSETS_QUERY
        .mark_matching_stale(
            &format!("api_wallet_account_assets:wallet={wallet_address};"),
            fresh_ttl,
        )
        .len()
}

pub(crate) fn mark_asset_detail_stale(
    address: &str,
    account_id: Option<u32>,
    chain_code: &str,
    token_address: Option<&str>,
    fresh_ttl: Duration,
) -> bool {
    let key = asset_detail_key(address, account_id, chain_code, token_address);
    ASSET_DETAIL_QUERY.mark_stale(&key, fresh_ttl)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::time::Duration;

    use crate::{
        error::service::ServiceError,
        response_vo::standard_wallet::{
            account::BalanceInfo,
            assets::{CoinAssets, GetAccountAssetsRes},
        },
    };

    use super::{
        account_assets_key, asset_detail_key, load_account_assets, load_asset_detail,
        mark_account_assets_stale, mark_asset_detail_stale,
    };

    fn unique_suffix() -> String {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string()
    }

    fn balance(amount: f64, fiat_value: f64) -> BalanceInfo {
        BalanceInfo {
            amount,
            currency: "USD".to_string(),
            unit_price: None,
            fiat_value: Some(fiat_value),
        }
    }

    fn account_assets(amount: f64) -> GetAccountAssetsRes {
        GetAccountAssetsRes { account_total_assets: balance(amount, amount) }
    }

    fn coin_assets(address: &str, chain_code: &str, token_address: &str) -> CoinAssets {
        CoinAssets {
            name: "Tether USD".to_string(),
            symbol: "USDT".to_string(),
            decimals: 6,
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_address.to_string(),
            status: 1,
            balance: balance(12.0, 12.0),
            is_multisig: 0,
            created_at: sqlx::types::chrono::Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn token_case_evm_detail_cache_keys_share_identity() {
        assert_eq!(
            asset_detail_key(
                "sender",
                Some(1),
                "polygon",
                Some("0xc2132D05D31c914a87C6611C10748AEb04B58e8F")
            ),
            asset_detail_key(
                "sender",
                Some(1),
                "polygon",
                Some("0xc2132d05d31c914a87c6611c10748aeb04b58e8f")
            )
        );
        assert_ne!(
            asset_detail_key("sender", Some(1), "tron", Some("TAbC")),
            asset_detail_key("sender", Some(1), "tron", Some("tabc"))
        );
    }

    #[test]
    fn account_assets_key_keeps_query_dimensions() {
        let key = account_assets_key("0xwallet", 7, Some("tron"));
        assert_eq!(key, "api_wallet_account_assets:wallet=0xwallet;account_id=7;chain_code=tron");
    }

    #[test]
    fn asset_detail_key_keeps_query_dimensions() {
        let key = asset_detail_key("TAddr", Some(1), "tron", Some("TToken"));
        assert_eq!(
            key,
            "api_wallet_asset_detail:address=TAddr;account_id=1;chain_code=tron;token=TToken"
        );
    }

    #[tokio::test]
    async fn account_assets_returns_stale_cache_on_timeout() {
        let wallet_address = format!("0xwallet{}", unique_suffix());
        let key = account_assets_key(&wallet_address, 3, Some("tron"));
        let fresh_ttl = Duration::from_secs(60);

        let first = load_account_assets(&key, fresh_ttl, || async { Ok(account_assets(88.0)) })
            .await
            .expect("initial account assets query ok");
        assert_eq!(first.account_total_assets.amount, 88.0);

        assert_eq!(mark_account_assets_stale(&wallet_address, fresh_ttl), 1);

        let stale = load_account_assets(&key, Duration::from_millis(0), || async {
            Err(ServiceError::Timeout)
        })
        .await
        .expect("timeout should return stale account assets");

        assert_eq!(stale.account_total_assets.amount, 88.0);
    }

    #[tokio::test]
    async fn asset_detail_returns_stale_cache_on_timeout() {
        let address = format!("TAddr{}", unique_suffix());
        let token_address = "TToken";
        let key = asset_detail_key(&address, None, "tron", Some(token_address));
        let fresh_ttl = Duration::from_secs(60);

        let first = load_asset_detail(&key, fresh_ttl, || async {
            Ok(coin_assets(&address, "tron", token_address))
        })
        .await
        .expect("initial asset detail query ok");
        assert_eq!(first.balance.amount, 12.0);

        assert!(mark_asset_detail_stale(&address, None, "tron", Some(token_address), fresh_ttl));

        let stale = load_asset_detail(&key, Duration::from_millis(0), || async {
            Err(ServiceError::Timeout)
        })
        .await
        .expect("timeout should return stale asset detail");

        assert_eq!(stale.address, address);
        assert_eq!(stale.balance.amount, 12.0);
    }
}
