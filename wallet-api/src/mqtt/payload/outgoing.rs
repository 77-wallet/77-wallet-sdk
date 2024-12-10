#[derive(Debug, serde::Serialize)]
pub(crate) enum OutgoingPayload {
    #[serde(untagged)]
    SwitchWallet {
        uid: String,
        sn: String,
        #[serde(default, rename = "appId")]
        app_id: String,
        #[serde(rename = "deviceType")]
        device_type: String,
        #[serde(rename = "clientId")]
        client_id: String,
    },
}

impl OutgoingPayload {
    pub(crate) fn to_vec(&self) -> Result<String, anyhow::Error> {
        let res = serde_json::to_string(&self)?;

        Ok(res)
    }
}
