use crate::{
    api::ReturnType,
    manager::WalletManager,
    response_vo::standard_wallet::{
        assets::{AccountChainAssetList, CoinAssets, GetAccountAssetsRes},
        chain::ChainList,
        coin::CoinInfoList,
    },
    service::asset::AssetsService,
};
use wallet_database::entities::asset_token_key::AssetTokenKey;

impl WalletManager {
    pub async fn add_coin(&self, req: crate::request::coin::AddCoinReq) -> ReturnType<()> {
        AssetsService::new(self.ctx)
            .add_coin_v2(&req.wallet_address, Some(req.account_id), req.chain_list, None)
            .await
    }

    pub async fn add_regular_coin(&self, req: crate::request::coin::AddCoinReq) -> ReturnType<()> {
        AssetsService::new(self.ctx)
            .add_coin_v2(&req.wallet_address, Some(req.account_id), req.chain_list, Some(false))
            .await
    }

    pub async fn add_multisig_coin(
        &self,
        req: crate::request::coin::AddMultisigCoinReq,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx)
            .add_coin_v2(&req.address, None, req.chain_list, Some(true))
            .await
    }

    pub async fn get_assets(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        _symbol: &str,
        token_address: Option<String>,
    ) -> ReturnType<CoinAssets> {
        let token_key = AssetTokenKey::from_raw(token_address.as_deref());
        AssetsService::new(self.ctx).detail(address, account_id, chain_code, token_key).await
    }

    pub async fn remove_coin(
        &self,
        wallet_address: &str,
        account_id: u32,
        chain_list: ChainList,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx)
            .remove_coin_v2(wallet_address, Some(account_id), chain_list, None)
            .await
    }

    pub async fn remove_regular_coin(
        &self,
        address: &str,
        chain_list: ChainList,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx).remove_coin_v2(address, None, chain_list, Some(false)).await
    }

    pub async fn remove_multisig_coin(
        &self,
        address: &str,
        chain_list: ChainList,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx).remove_coin_v2(address, None, chain_list, Some(true)).await
    }

    /// 获取普通账户已添加的币列表
    pub async fn get_coin_list(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> ReturnType<CoinInfoList> {
        AssetsService::new(self.ctx)
            .get_coin_list(address, account_id, chain_code, keyword, is_multisig)
            .await
    }

    pub async fn get_all_account_assets(
        &self,
        account_id: u32,
        wallet_address: Option<&str>,
    ) -> ReturnType<GetAccountAssetsRes> {
        AssetsService::new(self.ctx).get_all_account_assets(account_id, wallet_address).await
    }

    /// 获取普通账户总资产
    pub async fn get_account_assets(
        &self,
        account_id: u32,
        wallet_address: &str,
        chain_code: Option<String>,
    ) -> ReturnType<GetAccountAssetsRes> {
        AssetsService::new(self.ctx)
            .get_account_assets(account_id, wallet_address, chain_code)
            .await
    }

    /// 获取多签账户总资产
    pub async fn get_multisig_account_assets(
        &self,
        address: &str,
    ) -> ReturnType<GetAccountAssetsRes> {
        AssetsService::new(self.ctx).get_multisig_account_assets(address).await
    }

    pub async fn get_assets_list_v2(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
    ) -> ReturnType<AccountChainAssetList> {
        AssetsService::new(self.ctx)
            .get_account_chain_assets_v2(address, account_id, chain_code, is_multisig)
            .await
    }

    // 根据资产地址、后端同步。
    pub async fn sync_assets(
        &self,
        addr: String,
        chain_code: Option<String>,
        symbol: Vec<String>,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx).sync_assets_by_addr(vec![addr], chain_code, symbol).await
    }

    // 根据资产地址、链以及符号来同步余额(直接重链上同步余额)。
    pub async fn sync_balance_from_chain(
        &self,
        addr: String,
        chain_code: Option<String>,
        symbol: Vec<String>,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx).sync_assets_by_addr(vec![addr], chain_code, symbol).await
    }

    // 根据钱包去同步资产
    pub async fn sync_assets_by_wallet(
        &self,
        wallet_address: String,
        account_id: Option<u32>,
        symbol: Vec<String>,
    ) -> ReturnType<()> {
        AssetsService::new(self.ctx)
            .sync_assets_by_wallet_chain(wallet_address, account_id, symbol)
            .await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::{response_vo::standard_wallet::chain::ChainList, testkit::env::get_manager};
    use anyhow::Result;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_add_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let token_address = Some("0x55d398326f99059fF775485246999027B3197955".to_string());
        let account_id = 1;
        let add_coin_req = crate::request::coin::AddCoinReq {
            account_id,
            // symbol: "BNB".to_string(),
            // wallet_address: "0x57CF28DD99cc444A9EEEEe86214892ec9F295480".to_string(),
            wallet_address: "0xEB2b4F967D9a6BeA958dDe3e5814BbE33A5CBfE2".to_string(),
            chain_list: ChainList(HashMap::from([(
                "sol".to_string(),
                "So11111111111111111111111111111111111111112".to_string(),
            )])), // token_address,
        };
        let res = wallet_manager.add_coin(add_coin_req).await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_add_multisig_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let add_coin_req = crate::request::coin::AddMultisigCoinReq {
            chain_list: ChainList(HashMap::from([(
                "bnb".to_string(),
                "0x55d398326f99059fF775485246999027B3197955".to_string(),
            )])),
            address: "TBtrFfwCQEtR8HZ4hEgfmYWiVetMF11czn".to_string(),
            // token_address: token_address,
        };
        let res = wallet_manager.add_multisig_coin(add_coin_req).await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // wallet_manager.init_wallet_type("api").await?;
        let address = "0x868Bd024461e572555c26Ed196FfabAA475BFcCd";
        let account_id = Some(1);
        let chain_code = "ton";
        let symbol = "TON";
        let token_address = Some("".to_string());
        let res = wallet_manager
            .get_assets(address, account_id, chain_code, symbol, token_address)
            .await?;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_remove_coin() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // wallet_manager.init_wallet_type("api").await?;
        // let wallet_address = "0xd8dc4B7daEfc0C993d1A7d3E2D4Dc998436032b3";
        // let wallet_address = "0xa32D8B667Fd6d2e30C1E6D7fE6E4319Bf1D4D310";
        let wallet_address = "0xDA32fc1346Fa1DF9719f701cbdd6855c901027C1";
        let account_id = 1;
        // let symbol = "LTC";
        // let symbol = "BEANS";
        let res = wallet_manager
            .remove_coin(wallet_address, account_id, ChainList(HashMap::from([
                (
                    "tron".to_string(),
                    "TEkxiTehnzSmSe2XqrBj4w32RUN966rdz8".to_string(),
                ),
                (
                    "sui".to_string(),
                    "0x294de7579d55c110a00a7c4946e09a1b5cbeca2592fbb83fd7bfacba3cfeaf0e::drf::DRF"
                        .to_string(),
                ),
            ])))
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_remove_multisig_coin() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let res = wallet_manager
            .remove_multisig_coin("0x3bAc24b73c7A03C8715697cA1646a6f85B91023a", ChainList(HashMap::from([
                (
                    "tron".to_string(),
                    "TEkxiTehnzSmSe2XqrBj4w32RUN966rdz8".to_string(),
                ),
                (
                    "sui".to_string(),
                    "0x294de7579d55c110a00a7c4946e09a1b5cbeca2592fbb83fd7bfacba3cfeaf0e::drf::DRF"
                        .to_string(),
                ),
            ])))
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_coin_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let keyword = Some("t");
        let keyword = None;
        // let chain_code = Some("Tron");
        let chain_code = None;
        // let is_multisig = None;
        let is_multisig = Some(false);
        let wallet_address = "0xDA32fc1346Fa1DF9719f701cbdd6855c901027C1";

        // let account_id = Some(1);
        let account_id = Some(1);

        let res = wallet_manager
            .get_coin_list(wallet_address, account_id, chain_code, keyword, is_multisig)
            .await?;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {}", res);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_coin_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let keyword = Some("t");
        let keyword = None;
        let chain_code = Some("tron".to_string());
        // let chain_code = None;
        // let is_multisig = None;
        let is_multisig = Some(true);

        let address = "TRbHD77Y6WWDaz9X5esrVKwEVwRM4gTw6N";
        let res =
            wallet_manager.get_coin_list(address, None, chain_code, keyword, is_multisig).await?;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_open_account_pk() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) = get_manager().await?;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        // let address = "A9gBqKMQDWUYNiHHpHakSEsztKuxxN838EWGuG2WKc6F";
        let address = "0x0FDDDc2C86547328a0125468380d592d63625ba2";
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let account_id = 1;
        let res = wallet_manager
            .get_account_private_key(
                &test_params.create_wallet_req.wallet_password,
                address,
                account_id,
            )
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_all_account_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        let address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        let account_id = 2147483648;
        // let account_id = 1;
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let account_asset =
            wallet_manager.get_all_account_assets(account_id, Some(address)).await?;
        tracing::info!("account_asset: {account_asset:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        wallet_manager.set_currency("USD").await?;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        // let address = "0x9e2BEf062f301C85589E342d569058Fd4a1334d7";
        let address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let chain_code = Some("sol".to_string());
        // wallet_manager.init_wallet_type("api").await?;
        // let account_id = 2147483648;
        let account_id = 1;
        let account_asset =
            wallet_manager.get_account_assets(account_id, address, chain_code).await?;
        tracing::info!("account_asset: {account_asset:?}");
        let res = wallet_utils::serde_func::serde_to_string(&account_asset).unwrap();
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_account_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let address = "TT4QgNx2rVD35tYU1LJ6tH5Ya1bxmannBK";
        let account_asset = wallet_manager.get_multisig_account_assets(address).await?;
        tracing::info!("account_asset: {account_asset:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_assets_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let address = "0x531cCB9d552CBC5e16F0247b5657A5CDF2D77097";
        let address = "0x868Bd024461e572555c26Ed196FfabAA475BFcCd";
        let chain_code = None;
        let is_multisig = None;
        // let account_id = Some(1);

        let account_id = Some(1);

        wallet_manager.set_currency("USD").await?;
        let res =
            wallet_manager.get_assets_list_v2(address, account_id, chain_code, is_multisig).await?;
        // tracing::info!("get_account_chain_assets: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("get_account_chain_assets: {}", res);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_assets_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let address = "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo";
        // let chain_code = None;
        let chain_code = Some("tron".to_string());
        let is_multisig = Some(true);
        let account_id = None;

        let res =
            wallet_manager.get_assets_list_v2(address, account_id, chain_code, is_multisig).await?;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_sync_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let address = "TCZYpvSt36MXdnPhhFMyqwfpoqSHrjUg2b".to_string();
        let address = "TX3f4nAzaVsPJYNSxr3KYGDqEKTn6w599F".to_string();
        // let chain_code = None;
        let chain_code = None;
        let symbol = vec![];

        let res = wallet_manager.sync_assets(address, chain_code, symbol).await?;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_sync_assets_by_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let address = "TCZYpvSt36MXdnPhhFMyqwfpoqSHrjUg2b".to_string();
        let wallet_address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480".to_string();
        // let account_id = Some(1);
        let account_id = Some(2147483648);
        let symbol = vec![];

        let res = wallet_manager.sync_assets_by_wallet(wallet_address, account_id, symbol).await?;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
