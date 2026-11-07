use crate::{
    api::ReturnType,
    manager::WalletManager,
    response_vo::{
        api_wallet::assets::ApiAccountChainAssetList,
        standard_wallet::{
            account::{Balance, BalanceInfo},
            assets::{CoinAssets, GetAccountAssetsRes},
            chain::ChainList,
            coin::CoinInfoList,
        },
    },
    service::api_wallet::asset::ApiAssetsService,
};
use wallet_database::entities::asset_token_key::AssetTokenKey;

impl WalletManager {
    /// 获取某个api钱包总资产
    pub async fn get_api_wallet_assets(
        &self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> ReturnType<BalanceInfo> {
        ApiAssetsService::new(self.ctx)
            .get_api_wallet_assets(wallet_address, account_id, chain_code)
            .await
    }

    // pub async fn get_api_wallet_assets(&self, wallet_address: &str) -> ReturnType<BalanceInfo> {
    //     ApiAssetsService::new(self.ctx).get_api_wallet_assets(wallet_address).await
    // }

    /// 获取某个api钱包总资产v3
    pub async fn get_api_wallet_assets_v3(&self, wallet_address: &str) -> ReturnType<BalanceInfo> {
        ApiAssetsService::new(self.ctx).get_api_wallet_assets_v3(wallet_address, None, None).await
    }

    pub async fn get_api_assets_list_(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
    ) -> ReturnType<ApiAccountChainAssetList> {
        ApiAssetsService::new(self.ctx)
            .get_api_assets_list(wallet_address, account_id, chain_code, None)
            .await
    }

    // api钱包添加资产
    pub async fn api_add_assets(&self, req: crate::request::coin::AddCoinReq) -> ReturnType<()> {
        ApiAssetsService::new(self.ctx).add_assets(req).await
    }

    /// api钱包删除资产
    pub async fn api_remove_assets(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_list: ChainList,
    ) -> ReturnType<()> {
        ApiAssetsService::new(self.ctx)
            .remove_assets(wallet_address, account_id, chain_list, None)
            .await
    }

    // 已添加的币种列表
    pub async fn api_added_coin_list(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> ReturnType<CoinInfoList> {
        ApiAssetsService::new(self.ctx)
            .get_added_coin_list(address, account_id, chain_code, keyword, is_multisig)
            .await
    }

    // 根据钱包去同步资产
    pub async fn sync_api_assets_by_wallet(
        &self,
        wallet_address: String,
        account_id: Option<u32>,
        symbol: Vec<String>,
    ) -> ReturnType<()> {
        let res = ApiAssetsService::new(self.ctx)
            .sync_api_assets_by_wallet(wallet_address, account_id, symbol)
            .await;

        if let Err(e) = res {
            tracing::error!("sync_api_assets error: {}", e);
            return Err(e);
        }

        Ok(())
    }

    /// 查询链上的余额，并更新本地表
    pub async fn api_chain_balance(
        &self,
        address: String,
        chain_code: String,
        token_address: String,
    ) -> ReturnType<Balance> {
        ApiAssetsService::new(self.ctx).chain_balance(&address, &chain_code, &token_address).await
    }

    /// 资产列表
    pub async fn get_api_assets_list(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
        hide_zero_balance: bool,
    ) -> ReturnType<ApiAccountChainAssetList> {
        ApiAssetsService::new(self.ctx)
            .get_account_chain_assets_v2(
                address,
                account_id,
                chain_code,
                is_multisig,
                hide_zero_balance,
            )
            .await
    }

    /// 账户的总资产
    pub async fn get_api_account_assets(
        &self,
        account_id: u32,
        wallet_address: &str,
        chain_code: Option<String>,
    ) -> ReturnType<GetAccountAssetsRes> {
        ApiAssetsService::new(self.ctx)
            .get_account_assets(account_id, wallet_address, chain_code)
            .await
    }

    pub async fn get_api_assets(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        token_address: Option<String>,
    ) -> ReturnType<CoinAssets> {
        let token_key = AssetTokenKey::from_raw(token_address.as_deref());
        ApiAssetsService::new(self.ctx).detail(address, account_id, chain_code, token_key).await
    }
}
