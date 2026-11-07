use crate::{
    consts::endpoint::api_wallet::{
        ADDRESS_EXPAND_COMPLETE, ADDRESS_INIT, QUERY_ADDRESS_LIST, QUERY_ASSET_LIST,
    },
    request::api_wallet::address::*,
    response_vo::api_wallet::{
        Pages,
        address::{AssetsListRes, UsedAddressItem},
    },
};
use wallet_ecdh::GLOBAL_KEY;

use crate::{Error::ApiBackend, api::BackendApi, api_request::ApiBackendRequest};

impl BackendApi {
    // 地址初始化
    pub async fn expand_address(&self, req: &ApiAddressInitReq) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        // 1. 转换为后端视图，裁剪epoch
        let backend_req = ApiAddressInitReqBackend::from(req);
        // 2. 加密
        let api_req = ApiBackendRequest::new(&backend_req)?;
        let res = self.post_api_backend::<_, ()>(ADDRESS_INIT, api_req).await?;
        tracing::info!("res: {res:#?}");
        Ok(())
    }

    // 扩容完成上报
    pub async fn expand_address_complete(
        &self,
        req: ExpandAddressCompleteReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        let res = self.post_api_backend::<_, ()>(ADDRESS_EXPAND_COMPLETE, api_req).await?;
        tracing::debug!("[expand_address_complete] res: {res:#?}");
        Ok(())
    }

    // 查询已使用的地址列表
    pub async fn query_used_address_list(
        &self,
        req: &AddressListReq,
    ) -> Result<Pages<UsedAddressItem>, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        let opt =
            self.post_api_backend::<_, Pages<UsedAddressItem>>(QUERY_ADDRESS_LIST, api_req).await?;
        opt.ok_or(ApiBackend(999, Some("no address list".to_string())))
    }

    pub async fn query_asset_list(
        &self,
        req: &AssetListReq,
    ) -> Result<AssetsListRes, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        let res = self.post_api_backend::<_, AssetsListRes>(QUERY_ASSET_LIST, api_req).await?;
        res.ok_or(ApiBackend(999, Some("no asset list".to_string())))
    }
}
