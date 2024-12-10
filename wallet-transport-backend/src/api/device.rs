use super::BackendApi;
use crate::response::BackendResponse;

impl BackendApi {
    pub async fn device_init(
        &self,
        req: &crate::request::DeviceInitReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("device/init")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn device_delete(
        &self,
        req: &crate::request::DeviceDeleteReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("device/delete")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn device_bind_address(
        &self,
        req: &crate::request::DeviceBindAddressReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("device/bindAddress")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn device_unbind_address(
        &self,
        req: &crate::request::DeviceBindAddressReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("device/unBindAddress")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn keys_init(
        &self,
        req: &crate::request::KeysInitReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("keys/init")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    // report filter min value config
    pub async fn save_send_msg_account(
        &self,
        req: crate::response_vo::app::SaveSendMsgAccount,
    ) -> Result<(), crate::Error> {
        self.client
            .post("device/saveSendMsgAmount")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?
            .process()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        api::BackendApi,
        request::{
            DeviceBindAddressReq, DeviceDeleteReq, DeviceInitReq, DeviceUnbindAddress, KeysInitReq,
        },
    };
    use wallet_utils::init_test_log;

    #[tokio::test]
    async fn test_device_init() {
        let base_url = crate::consts::BASE_URL;

        let req = DeviceInitReq {
            device_type: "ANDROID".to_string(),
            sn: "dead3430844c05f837d2301d7b3bc2f3".to_string(),
            code: "3".to_string(),
            system_ver: "4".to_string(),
            iemi: Some("5".to_string()),
            meid: Some("6".to_string()),
            iccid: Some("7".to_string()),
            mem: Some("8".to_string()),
            // app_id: Some("9".to_string()),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .device_init(&req)
            .await;

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_device_delete() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = DeviceDeleteReq {
            sn: "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3".to_string(),
            uid_list: vec!["c7c8453fbf4368279c822a1c39f3c955".to_string()],
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .device_delete(&req)
            .await;

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_device_bind_address() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = DeviceBindAddressReq {
            sn: "14ae14461d0891116eb85ef447ecb28dc22781d987b5cb0f75f8d3bcca18ebed".to_string(),
            address: vec![DeviceUnbindAddress {
                chain_code: "tron".to_string(),
                address: "TFzMRRzQFhY9XFS37veoswLRuWLNtbyhiB".to_string(),
            }],
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .device_bind_address(&req)
            .await;

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_device_unbind_address() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = DeviceBindAddressReq {
            sn: "bdb6412a9cb4b12c48ebe1ef4e9f052b07af519b7485cd38a95f38d89df97cb8".to_string(),
            address: vec![DeviceUnbindAddress {
                chain_code: "tron".to_string(),
                address: "TFzMRRzQFhY9XFS37veoswLRuWLNtbyhiB".to_string(),
            }],
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .device_unbind_address(&req)
            .await;

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_keys_init() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = KeysInitReq {
            uid: "dead3430844c05f837d2301d7b3bc2f3".to_string(),
            sn: "dead3430844c05f837d2301d7b3bc2f3".to_string(),
            client_id: Some("guanxiang".to_string()),
            device_type: Some("ANDROID".to_string()),
            app_id: Some("asad".to_string()),
            name: "asad".to_string(),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .keys_init(&req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_main() -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder().build()?;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse()?);
        headers.insert("Cookie", "sl-session=/hUSdIIxfWY+3tUC5NQC9A==".parse()?);

        let data = r#"{
            "deviceType": "ANDROID",
            "sn": "2",
            "code": "2",
            "systemVer": "3",
            "iemi": "4",
            "meid": "5",
            "iccid": "6",
            "mem": "7"
        }"#;

        let json: serde_json::Value = serde_json::from_str(&data)?;

        let request = client
            .request(reqwest::Method::POST, "http://api.wallet.net/device/init")
            .headers(headers)
            .json(&json);

        let response = request.send().await?;
        let body = response.text().await?;

        println!("{}", body);

        Ok(())
    }
}
