use crate::{
    api::BackendApi,
    response::response::BackendResponse,
    response_vo::app::{AllConfig, FindConfigByKeyRes},
};

impl BackendApi {
    pub async fn find_config_by_key(
        &self,

        req: crate::request::FindConfigByKey,
    ) -> Result<FindConfigByKeyRes, crate::Error> {
        let res = self
            .client
            .post("sys/config/findConfigByKey")
            .json(req)
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn all_config(&self) -> Result<AllConfig, crate::Error> {
        let res = self.client.post("sys/config/findConfigs").send::<serde_json::Value>().await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process(&self.aes_cbc_cryptor)
    }
}
