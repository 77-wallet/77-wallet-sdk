use crate::{
    consts::endpoint::api_wallet::RESOURCE_DELEGATION_APPLY,
    request::api_wallet::resource_delegation::*,
};
use wallet_ecdh::GLOBAL_KEY;

use crate::{api::BackendApi, api_request::ApiBackendRequest};

impl BackendApi {
    pub async fn apply_resource_delegation(
        &self,
        req: &ResourceApplyReq,
    ) -> Result<ApplyResourceDlRep, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        let resp = self
            .post_api_backend::<_, ApplyResourceDlRep>(RESOURCE_DELEGATION_APPLY, api_req)
            .await?;
        resp.ok_or_else(|| {
            crate::Error::Backend(Some("resource delegation apply response is empty".to_string()))
        })
    }
}
