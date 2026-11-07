use crate::{
    api::BackendApi,
    request::{SignedTranAcceptReq, SignedTranCreateReq, SignedTranUpdateHashReq, SyncBillReq},
    response::response::BackendResponse,
    response_vo::{chain::GasOracle, transaction::RecordResp},
};
use std::collections::HashMap;

impl BackendApi {
    pub async fn signed_tran_create(
        &self,

        req: &SignedTranCreateReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("signed/trans/create")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn signed_tran_accept(
        &self,

        req: &SignedTranAcceptReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("signed/trans/accept")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn signed_tran_update_trans_hash(
        &self,

        req: &SignedTranUpdateHashReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("signed/trans/updateTransdHash")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn gas_oracle(&self, chain_code: &str) -> Result<GasOracle, crate::Error> {
        let mut params = HashMap::new();
        params.insert("chainCode", chain_code);

        let res =
            self.client.post("token/findGasTracker").json(params).send::<BackendResponse>().await?;

        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn record_lists(
        &self,

        chain_code: &str,
        address: &str,
        start_time: Option<String>,
    ) -> Result<RecordResp, crate::Error> {
        let req = SyncBillReq::new(chain_code, address, start_time);

        let res =
            self.client.post("account/record/list").json(req).send::<BackendResponse>().await?;

        res.process(&self.aes_cbc_cryptor)
    }
}
