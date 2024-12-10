use wallet_database::entities::assets::AssetsEntity;
use wallet_types::chain::address::category::AddressCategory;

#[derive(Debug, serde::Serialize)]
pub struct GeneratePhraseRes {
    pub phrases: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct QueryPhraseRes {
    pub phrases: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateWalletRes {
    pub address: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetRootRes {
    pub wallet_tree: wallet_tree::wallet_tree::WalletTree,
}

#[derive(Debug, serde::Serialize)]
pub struct GetPhraseRes {
    pub phrase: String,
}

// #[derive(Debug, serde::Serialize, Default)]
// pub struct GetWalletListRes {
//     pub wallet_list: std::collections::HashMap<String, WalletInfo>,
// }

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletInfo {
    pub address: String,
    pub uid: String,
    pub name: String,
    pub balance: super::account::BalanceInfo,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    pub account_list: AccountInfos,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub account_id: u32,
    pub account_index_map: wallet_utils::address::AccountIndexMap,
    pub name: String,
    pub balance: super::account::BalanceInfo,
    pub chain: Vec<ChainInfo>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    pub address: String,
    pub wallet_address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub name: Option<String>,
    pub address_type: AddressCategory,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfos(pub Vec<AccountInfo>);

impl std::ops::Deref for AccountInfos {
    type Target = Vec<AccountInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for AccountInfos {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AccountAssetsMap {
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    pub chain_code: String,
    pub token_address: String,
    pub protocol: Option<String>,
    pub balance: String,
}

// TODO: 没改完
#[derive(Debug, serde::Serialize)]
pub struct AccountAssetsMaps(Vec<AccountAssetsMap>);

impl std::ops::Deref for AccountAssetsMaps {
    type Target = Vec<AccountAssetsMap>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for AccountAssetsMaps {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<AssetsEntity>> for AccountAssetsMaps {
    fn from(value: Vec<AssetsEntity>) -> Self {
        let mut account_assets_map = Vec::<AccountAssetsMap>::new();
        for asset in value {
            account_assets_map.push(AccountAssetsMap {
                symbol: asset.symbol,
                decimals: asset.decimals,
                address: asset.address,
                chain_code: asset.chain_code,
                token_address: asset.token_address,
                protocol: asset.protocol,
                balance: asset.balance,
            })
        }
        AccountAssetsMaps(account_assets_map)
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ImportDerivationPathRes {
    pub accounts: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDerivationPathRes {
    pub file_path: String,
}
