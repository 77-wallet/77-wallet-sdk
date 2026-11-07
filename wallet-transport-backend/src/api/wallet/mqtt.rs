use crate::{api::BackendApi, response::response::BackendResponse};

impl BackendApi {
    pub async fn send_msg_confirm(
        &self,

        req: &crate::request::SendMsgConfirmReq,
    ) -> Result<(), crate::Error> {
        let res = self
            .client
            .post("sendMsg/confirm")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn query_unconfirm_msg(
        &self,
        req: &crate::request::QueryUnconfirmMsgReq,
    ) -> Result<crate::response_vo::mqtt::UnconfirmMsgRes, crate::Error> {
        let res = self
            .client
            .post("sendMsg/queryUnConfirmMsg")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn get_unconfirm_by_msg_id(
        &self,

        req: &crate::request::GetUnconfirmById,
    ) -> Result<crate::response_vo::mqtt::UnconfirmMsgResp, crate::Error> {
        let res = self
            .client
            .post("sendMsg/getMsgById")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }
}
