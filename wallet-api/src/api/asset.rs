use crate::api::ReturnType;
use crate::response_vo::assets::{
    AccountChainAsset, AccountChainAssetList, GetAccountAssetsRes, GetChainAssetsRes,
};
use crate::service::asset::AssetsService;

impl crate::WalletManager {
    pub async fn add_coin(&self, req: crate::request::coin::AddCoinReq) -> ReturnType<()> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .add_coin(
                &req.wallet_address,
                Some(req.account_id),
                &req.symbol,
                req.chain_code,
                None,
            )
            .await?
            .into()
    }

    pub async fn add_regular_coin(&self, req: crate::request::coin::AddCoinReq) -> ReturnType<()> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .add_coin(
                &req.wallet_address,
                Some(req.account_id),
                &req.symbol,
                req.chain_code,
                Some(false),
            )
            .await?
            .into()
    }

    pub async fn add_multisig_coin(
        &self,
        req: crate::request::coin::AddMultisigCoinReq,
    ) -> ReturnType<()> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .add_coin(&req.address, None, &req.symbol, None, Some(true))
            .await?
            .into()
    }

    pub async fn get_assets(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        symbol: &str,
    ) -> ReturnType<crate::response_vo::assets::CoinAssets> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .detail(address, account_id, chain_code, symbol)
            .await?
            .into()
    }

    pub async fn remove_coin(
        &self,
        wallet_address: &str,
        account_id: u32,
        symbol: &str,
    ) -> ReturnType<()> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .remove_coin(wallet_address, Some(account_id), symbol, None)
            .await?
            .into()
    }

    pub async fn remove_regular_coin(&self, address: &str, symbol: &str) -> ReturnType<()> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .remove_coin(address, None, symbol, Some(false))
            .await?
            .into()
    }

    pub async fn remove_multisig_coin(&self, address: &str, symbol: &str) -> ReturnType<()> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .remove_coin(address, None, symbol, Some(true))
            .await?
            .into()
    }

    /// 获取普通账户已添加的币列表
    pub async fn get_coin_list(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> ReturnType<crate::response_vo::coin::CoinInfoList> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_coin_list(address, account_id, chain_code, keyword, is_multisig)
            .await?
            .into()
    }

    #[deprecated]
    pub async fn get_account_assets_by_symbol_and_chain_code(
        &self,
        account_address: &str,
        chain_code: &str,
        symbol: &str,
    ) -> ReturnType<AccountChainAsset> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_account_assets_by_symbol_and_chain_code(account_address, chain_code, symbol)
            .await?
            .into()
    }

    pub async fn get_all_account_assets(
        &self,
        account_id: u32,
        wallet_address: Option<&str>,
    ) -> ReturnType<GetAccountAssetsRes> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_all_account_assets(account_id, wallet_address)
            .await?
            .into()
    }

    // TODO:
    /// 获取普通账户总资产
    pub async fn get_account_assets(
        &self,
        account_id: u32,
        wallet_address: &str,
    ) -> ReturnType<GetAccountAssetsRes> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_account_assets(account_id, wallet_address)
            .await?
            .into()
    }

    /// 获取多签账户总资产
    pub async fn get_multisig_account_assets(
        &self,
        address: &str,
    ) -> ReturnType<GetAccountAssetsRes> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_multisig_account_assets(address)
            .await?
            .into()
    }

    /// 获取网络资产
    #[deprecated]
    pub async fn get_chain_assets(
        &self,
        // wallet_name: &str,
        address: &str,
        get_chain: crate::request::assets::GetChain,
    ) -> ReturnType<GetChainAssetsRes> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_chain_assets(address, get_chain)
            .await?
            .into()
    }

    pub async fn get_assets_list_v2(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
    ) -> ReturnType<AccountChainAssetList> {
        AssetsService::new(self.repo_factory.resuource_repo())
            .get_account_chain_assets_v2(address, account_id, chain_code, is_multisig)
            .await?
            .into()
    }

    // 同步资产
    pub async fn sync_assets(
        &self,
        addr: Vec<String>,
        chain_code: Option<String>,
        symbol: Vec<String>,
    ) -> ReturnType<()> {
        let res = AssetsService::new(self.repo_factory.resuource_repo())
            .sync_assets_by_addr(addr, chain_code, symbol)
            .await;
        if let Err(e) = res {
            tracing::error!("sync_assets error: {}", e);
        }

        ().into()
    }

    // 根据钱包去同步资产
    pub async fn sync_assets_by_wallet(
        &self,
        wallet_address: String,
        account_id: Option<u32>,
        symbol: Vec<String>,
    ) -> ReturnType<()> {
        let res = AssetsService::new(self.repo_factory.resuource_repo())
            .sync_assets_by_wallet(wallet_address, account_id, symbol)
            .await;
        if let Err(e) = res {
            tracing::error!("sync_assets error: {}", e);
        }

        ().into()
    }
}

#[cfg(test)]
mod test {
    use crate::test::env::{setup_test_environment, TestData};
    use anyhow::Result;

    #[tokio::test]
    async fn test_add_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let add_coin_req = crate::request::coin::AddCoinReq {
            account_id: 1,
            symbol: "WIN".to_string(),
            wallet_address: "0x35159Ffd3Ba9b8748E16A929612e5212a4394F48".to_string(),
            chain_code: None,
        };
        let res = wallet_manager.add_coin(add_coin_req).await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let address = "TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF";
        let account_id = None;
        let chain_code = "tron";
        let symbol = "TRX";
        let res = wallet_manager
            .get_assets(address, account_id, chain_code, symbol)
            .await;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_remove_coin() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let wallet_address = "0xd8dc4B7daEfc0C993d1A7d3E2D4Dc998436032b3";
        // let wallet_address = "0xa32D8B667Fd6d2e30C1E6D7fE6E4319Bf1D4D310";
        let wallet_address = "0x8E5424c1347d27B6816eba3AEE7FbCeDFa229C1F";
        // let symbol = "LTC";
        // let symbol = "BEANS";
        let symbol = "USDT";
        let res = wallet_manager.remove_coin(wallet_address, 1, symbol).await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_remove_multisig_coin() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let res = wallet_manager
            .remove_multisig_coin("TT4QgNx2rVD35tYU1LJ6tH5Ya1bxmannBK", "SUN")
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_assets_by_symbol_and_chain_code() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let symbol = "TRX";
        let chain_code = "tron";
        let res = wallet_manager
            .get_account_assets_by_symbol_and_chain_code(
                "TLbFepwLNd372mSQGhPCPxqZBRQ8zsCLav",
                chain_code,
                symbol,
            )
            .await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_coin_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let keyword = Some("t");
        let keyword = None;
        // let chain_code = Some("Tron");
        let chain_code = None;
        // let is_multisig = None;
        let is_multisig = Some(false);
        let wallet_address = "0x35159Ffd3Ba9b8748E16A929612e5212a4394F48";
        let res = wallet_manager
            .get_coin_list(wallet_address, Some(1), chain_code, keyword, is_multisig)
            .await;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_coin_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let keyword = Some("t");
        let keyword = None;
        let chain_code = Some("tron".to_string());
        // let chain_code = None;
        // let is_multisig = None;
        let is_multisig = Some(true);

        let address = "TRbHD77Y6WWDaz9X5esrVKwEVwRM4gTw6N";
        let res = wallet_manager
            .get_coin_list(address, None, chain_code, keyword, is_multisig)
            .await;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_open_account_pk() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env,
            ..
        } = setup_test_environment(None, None, false, None).await?;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        // let address = "A9gBqKMQDWUYNiHHpHakSEsztKuxxN838EWGuG2WKc6F";
        let address = "0x0FDDDc2C86547328a0125468380d592d63625ba2";
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let account_id = 1;
        let res = wallet_manager
            .get_account_private_key(&wallet_env.password, address, account_id)
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_chain_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        // let address = "A9gBqKMQDWUYNiHHpHakSEsztKuxxN838EWGuG2WKc6F";
        let address = "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB";
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let res = wallet_manager
            .get_chain_assets(address, crate::request::assets::GetChain::All)
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_all_account_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        let address = "0xa32D8B667Fd6d2e30C1E6D7fE6E4319Bf1D4D310";
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let account_asset = wallet_manager
            .get_all_account_assets(1, Some(address))
            .await;
        tracing::info!("account_asset: {account_asset:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        wallet_manager.set_currency("USD").await;
        // let address = "TCWBCCuapMcnrSxhudiNshq1UK4nCvZren";
        // let address = "0x9e2BEf062f301C85589E342d569058Fd4a1334d7";
        let address = "0x35159Ffd3Ba9b8748E16A929612e5212a4394F48";
        // let address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let account_asset = wallet_manager.get_account_assets(1, address).await;
        tracing::info!("account_asset: {account_asset:?}");
        let res = wallet_utils::serde_func::serde_to_string(&account_asset).unwrap();
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_account_assets() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let address = "TT4QgNx2rVD35tYU1LJ6tH5Ya1bxmannBK";
        let account_asset = wallet_manager.get_multisig_account_assets(address).await;
        tracing::info!("account_asset: {account_asset:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_assets_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        // let address = "0x531cCB9d552CBC5e16F0247b5657A5CDF2D77097";
        let address = "0x8E5424c1347d27B6816eba3AEE7FbCeDFa229C1F";
        // let chain_code = Some("bnb");
        // let chain_code = Some("btc");
        // let chain_code = Some("eth".to_string());
        // let chain_code = Some("tron".to_string());
        let chain_code = None;
        // let is_multisig = Some(false);
        let is_multisig = None;
        let account_id = Some(1);
        wallet_manager.set_currency("USD").await;
        let res = wallet_manager
            .get_assets_list_v2(address, account_id, chain_code, is_multisig)
            .await;
        tracing::info!("get_account_chain_assets: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("get_account_chain_assets: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_assets_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let address = "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo";
        // let chain_code = None;
        let chain_code = Some("tron".to_string());
        let is_multisig = Some(true);
        let account_id = None;

        let res = wallet_manager
            .get_assets_list_v2(address, account_id, chain_code, is_multisig)
            .await;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
