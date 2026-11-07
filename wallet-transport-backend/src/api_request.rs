use wallet_ecdh::GLOBAL_KEY;

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct ApiBackendRequestBody {
    pub key: String,
    pub data: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct ApiBackendRequest {
    pub sn: String,
    pub sign: String,
    pub body: ApiBackendRequestBody,
}

impl ApiBackendRequest {
    pub fn new<T>(req: T) -> Result<Self, crate::Error>
    where
        T: serde::Serialize,
    {
        let req_data = serde_json::json!(req);
        let d = GLOBAL_KEY.encrypt(req_data.to_string().as_bytes())?; // base64
        let key = wallet_utils::bytes_to_base64(&d.key);
        let ct = wallet_utils::bytes_to_base64(&d.ciphertext);
        // tracing::info!("Request encrypt 2, key: {:?}, ct: {:?}", key, ct);
        let body = ApiBackendRequestBody { key: key.clone(), data: ct.clone() };
        // tracing::info!("Request encrypt 3 body : {:?}", body);

        // 签名
        let tag = uuid::Uuid::new_v4().to_string();
        let body_data = key + ct.as_str();
        // tracing::info!("Request sign 4: {:?}", body_data);
        let vec_sign = GLOBAL_KEY.sign(&tag, body_data.as_bytes())?;
        let sign = wallet_utils::bytes_to_base64(&vec_sign);
        // tracing::info!("Request sign 5 sig: {:?}", sign);
        let api_req = ApiBackendRequest { sn: GLOBAL_KEY.sn().to_string(), sign, body };
        // tracing::info!("Request sign 6 api req: {:?}", wallet_utils::serde_func::serde_to_string(&api_req)?);
        Ok(api_req)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiBackendRequest;
    use serial_test::serial;
    use wallet_ecdh::GLOBAL_KEY;

    const TEST_PUB_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEWDZNP0ClbeWJey9hBr2rsjSayQEBywnv
ZXi0RberQCAp+06fOjvr+jZI5qwYGglmMkGJw49tbni6qgm4QNV6WQ==
-----END PUBLIC KEY-----"#;

    fn setup_crypto(sn: &str) -> Result<(), crate::Error> {
        GLOBAL_KEY.set_shared_secret(TEST_PUB_KEY)?;
        GLOBAL_KEY.set_sn(sn);
        Ok(())
    }

    #[serial]
    #[test]
    fn api_backend_request_new_sets_all_required_fields() -> Result<(), crate::Error> {
        setup_crypto("test-sn-001")?;
        let req = ApiBackendRequest::new(serde_json::json!({
            "uid": "u-1",
            "orgAppId": "app-1"
        }))?;

        assert_eq!(req.sn, "test-sn-001");
        assert!(!req.sign.is_empty());
        assert!(!req.body.key.is_empty());
        assert!(!req.body.data.is_empty());
        Ok(())
    }
}
