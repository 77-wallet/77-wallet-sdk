use crate::{response::response::BackendResponse, response_vo::announcement::BulletinInfoList};

use crate::api::BackendApi;

impl BackendApi {
    pub async fn announcement_list(
        &self,
        req: crate::request::AnnouncementListReq,
    ) -> Result<BulletinInfoList, crate::Error> {
        let res = self.client.post("bulletin/list").json(req).send::<serde_json::Value>().await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process(&self.aes_cbc_cryptor)
    }
}
