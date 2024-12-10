pub struct ServiceFeePayer {
    pub from: String,
    pub chain_code: String,
    pub symbol: String,
    pub fee_setting: Option<String>,
    pub request_resource_id: Option<String>,
}

pub struct DeployFeePayer {
    pub account_id: String,
    pub fee_setting: String,
}

pub struct Executor {
    pub address: String,
    pub password: String,
}
