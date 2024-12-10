use super::BackendApi;
use crate::response::BackendResponse;

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

        res.process()
    }

    pub async fn send_msg_query_unconfirm_msg(
        &self,
        req: &crate::request::SendMsgQueryUnconfirmMsgReq,
    ) -> Result<crate::response_vo::mqtt::SendMsgQueryUnconfirmMsgRes, crate::Error> {
        let res = self
            .client
            .post("sendMsg/queryUnConfirmMsg")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }
}

#[cfg(test)]
mod test {

    use wallet_utils::init_test_log;

    use crate::{
        api::BackendApi,
        request::{SendMsgConfirm, SendMsgConfirmReq},
    };

    #[tokio::test]
    async fn test_send_msg_confirm() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = SendMsgConfirmReq {
            list: vec![SendMsgConfirm {
                id: "670a450c19240906131ac1cf".to_string(),
                source: "MQTT".to_string(),
            }],
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .send_msg_confirm(&req)
            .await;

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_send_msg_query_unconfirm_msg() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;
        let client_id = "86d70879ca5ace5474313fd309e68f37".to_string();
        let req = crate::request::SendMsgQueryUnconfirmMsgReq { client_id };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .send_msg_query_unconfirm_msg(&req)
            .await;
        println!("[test_chain_default_list] res: {res:?}");
    }
}
