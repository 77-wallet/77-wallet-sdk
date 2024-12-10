use super::BackendApi;
use crate::{request::LanguageInitReq, response::BackendResponse};

impl BackendApi {
    pub async fn app_install_save(
        &self,
        req: crate::request::AppInstallSaveReq,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>, crate::Error> {
        let res = self
            .client
            .post("/app/install/save")
            .json(serde_json::json!(req))
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn app_install_download(&self) -> Result<String, crate::Error> {
        let res = self
            .client
            .post("/app/install/download")
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn mqtt_init(&self) -> Result<serde_json::Value, crate::Error> {
        let res = self
            .client
            .post("mqtt/init")
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn rpc_token(&self, client_id: &str) -> Result<String, crate::Error> {
        self.client
            .post("app/rpc/token")
            .json(serde_json::json!({"clientId":client_id}))
            .send::<BackendResponse>()
            .await?
            .process()
    }

    pub async fn version_view(
        &self,
        req: crate::request::VersionViewReq,
    ) -> Result<crate::response_vo::app::AppVersionRes, crate::Error> {
        let res = self
            .client
            .post("/version/view")
            .json(serde_json::json!(req))
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn version_his_version(
        &self,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>, crate::Error> {
        let res = self
            .client
            .post("/version/hisVersion")
            // .json(serde_json::json!(req))
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn version_download_url(
        &self,
        url: &str,
    ) -> Result<crate::response_vo::app::AppVersionRes, crate::Error> {
        let res = self
            .client
            .get(&format!("version/download/{url}"))
            .send::<serde_json::Value>()
            .await?;
        let res: BackendResponse = wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn language_init(&self, req: LanguageInitReq) -> Result<(), crate::Error> {
        let res = self
            .client
            .post("/language/init")
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

    use crate::{
        api::BackendApi,
        request::{AppInstallSaveReq, LanguageInitReq, VersionViewReq},
    };

    #[tokio::test]
    async fn test_app_install_save() {
        let base_url = "http://api.wallet.net";

        let req = AppInstallSaveReq {
            sn: "1".to_string(),
            channel: None,
            device_type: Some("ANDROID".to_string()),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .app_install_save(req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_app_install_download() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .app_install_download()
            .await
            .unwrap();

        println!("[test_app_install_download] res: {res:?}");
    }

    #[tokio::test]
    async fn test_version_his_version() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .version_his_version()
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_token() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .rpc_token("6f88a37aca2384cec6029d5983fac0e2")
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_version_view() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let device_type = "ANDROID";
        let r#type = Some("android_google_shop".to_string());
        let req = VersionViewReq::new(device_type, r#type);
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .version_view(req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_version_download_url() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let url = "https://77.im/#/download";
        let encode_url = urlencoding::encode(url);
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .version_download_url(&encode_url)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }
    #[tokio::test]
    async fn test_language_init() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = LanguageInitReq {
            client_id: "6f88a37aca2384cec6029d5983fac0e2".to_string(),
            lan: "CHINESE_SIMPLIFIED".to_string(),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .language_init(req)
            .await
            .unwrap();

        println!("[test_language_init] res: {res:?}");
    }

    #[tokio::test]
    async fn test_mqtt_init() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .mqtt_init()
            .await
            .unwrap();

        println!("[test_language_init] res: {res:?}");
    }
}
