#[derive(Debug, Clone)]
pub struct BindForm {
    pub app_id: String,
    pub org_id: String,
    pub subaccount_uid: String,
    pub withdrawal_uid: String,
}

impl Default for BindForm {
    fn default() -> Self {
        Self {
            app_id: "f2a904c3c12e4481bbabb86977c200b3".to_string(),
            org_id: "6933cf7a7fec37621a3ffc95".to_string(),
            subaccount_uid: "8fa020e0049b10e467fd21ea81b45bf44b88eaec3db8f167173760fc63cf9c90"
                .to_string(),
            withdrawal_uid: "f64db1f0796fa815016a067dceb9f912b77ec96ad79dd201534b82e905a1f29a"
                .to_string(),
        }
    }
}

impl BindForm {
    pub fn for_client(client_name: &str) -> Self {
        match client_name {
            "client4" => Self {
                app_id: "8276baee61e14956bf8ad036e4a5efb3".to_string(),
                org_id: "6a044edb3f923904b04aaf71".to_string(),
                subaccount_uid: "ef98e62f7057e2c6cee9314ee017875b283dccaaeeeabc9370f8afa7a3a5e186"
                    .to_string(),
                withdrawal_uid: "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e"
                    .to_string(),
            },
            _ => Self::default(),
        }
    }

    pub fn app_id(&self) -> String {
        self.app_id.trim().to_string()
    }

    pub fn org_id(&self) -> String {
        self.org_id.trim().to_string()
    }

    pub fn subaccount_uid(&self) -> String {
        self.subaccount_uid.trim().to_string()
    }

    pub fn withdrawal_uid(&self) -> String {
        self.withdrawal_uid.trim().to_string()
    }
}
