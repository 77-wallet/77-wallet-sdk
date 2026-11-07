use crate::{
    response::response::BackendResponse,
    response_vo::address::{AddressDetailsList, AssertResp},
};

use crate::api::BackendApi;

impl BackendApi {
    pub async fn address_init(
        &self,
        req: &crate::request::AddressInitReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("address/init")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn address_batch_init(
        &self,
        req: &crate::request::AddressBatchInitReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("address/batch/init")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn address_update_account_name(
        &self,
        req: &crate::request::AddressUpdateAccountNameReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("address/updateAccountName")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn address_find_multisiged_details(
        &self,
        req: crate::request::AddressDetailsReq,
    ) -> Result<AddressDetailsList, crate::Error> {
        let res = self
            .client
            .post("/address/findMultiSignedDetails")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    // 地址的资产 uid--> 钱包uid,
    pub async fn wallet_assets_list(
        &self,
        uid: String,
        index: Option<u32>,
    ) -> Result<AssertResp, crate::Error> {
        let req = serde_json::json!({
            "uid":uid,
            "index":index,
        });

        let res =
            self.client.post("wallet/assets/list").json(req).send::<BackendResponse>().await?;

        res.process(&self.aes_cbc_cryptor)
    }

    // 地址 - 链获取余额
    pub async fn wallet_assets_chain_list(
        &self,
        address: &str,
        chain_code: &str,
    ) -> Result<AssertResp, crate::Error> {
        let req = serde_json::json!({
            "address":address,
            "chainCode":chain_code,
        });

        let res = self
            .client
            .post("wallet/assets/chain/list")
            .json(req)
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    // 告知后端需要刷新余额
    pub async fn wallet_assets_refresh_bal(
        &self,
        address: &str,
        chain_code: &str,
        symbol: &str,
    ) -> Result<Option<()>, crate::Error> {
        let req = serde_json::json!({
            "address":address,
            "chainCode":chain_code,
            "symbol":symbol,
        });

        let res = self
            .client
            .post("wallet/assets/refreshAddressBalance")
            .json(req)
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }
}
