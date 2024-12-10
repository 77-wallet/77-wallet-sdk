use crate::{response::BackendResponse, response_vo::app::FindConfigByKeyRes};

use super::BackendApi;

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
        res.process()
    }
}

#[cfg(test)]
mod test {

    use crate::{api::BackendApi, request::FindConfigByKey};

    #[tokio::test]
    async fn test_find_config_by_key() {
        let base_url = crate::consts::BASE_URL;

        let req = FindConfigByKey {
            key: "OFFICIAL:WEBSITE".to_string(),
        };

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .find_config_by_key(req)
            .await
            .unwrap();

        println!("[find_config_by_key] res: {res:?}");
    }
}
