use crate::{
    api::BackendApi,
    consts::endpoint::api_wallet::INIT_SWAP,
    request::api_wallet::swap::{ApiInitSwapReq, ApiInitSwapResponse},
};

impl BackendApi {
    // 地址初始化
    pub async fn init_swap(
        &self,
        req: &ApiInitSwapReq,
    ) -> Result<ApiInitSwapResponse, crate::Error> {
        // 1. 加密
        let res = self.client.post(INIT_SWAP).json(req).send::<ApiInitSwapResponse>().await?;
        Ok(res)
    }
}
