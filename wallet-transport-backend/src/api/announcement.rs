use crate::{response::BackendResponse, response_vo::announcement::BulletinInfoList};

use super::BackendApi;

impl BackendApi {
    pub async fn announcement_list(
        &self,
        req: crate::request::AnnouncementListReq,
    ) -> Result<BulletinInfoList, crate::Error> {
        let res = self
            .client
            .post("bulletin/list")
            .json(req)
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }
}

#[cfg(test)]
mod test {

    use wallet_utils::init_test_log;

    use crate::{api::BackendApi, request::AnnouncementListReq};

    #[tokio::test]
    async fn test_announcement_list() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = AnnouncementListReq {
            client_id: "6f88a37aca2384cec6029d5983fac0e2".to_string(),
            order_column: "create_time".to_string(),
            page_num: 0,
            page_size: 1000,
        };
        let req_str = wallet_utils::serde_func::serde_to_string(&req).unwrap();
        tracing::info!("req_str: {req_str}");
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .announcement_list(req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:#?}");
    }
}
