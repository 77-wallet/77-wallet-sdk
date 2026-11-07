pub mod account;
pub mod api_wallet;
pub mod app;
pub mod assets;
pub mod coin;
pub mod config;
pub mod devices;
pub mod init;
pub mod permission;
pub mod stake;
pub mod transaction;
pub mod wallet;

#[derive(serde::Deserialize, Debug)]
pub struct WalletReq {
    pub address: String,
    pub account_id: u32,
}

#[derive(serde::Deserialize, Debug)]
pub struct AccountReq {
    pub address: String,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
pub enum AccountRequest {
    Wallet(WalletReq),
    Account(AccountReq),
}

impl TryFrom<(Option<String>, Option<u32>)> for AccountRequest {
    fn try_from(value: (Option<String>, Option<u32>)) -> Result<Self, Self::Error> {
        match value {
            (Some(address), Some(account_id)) => {
                Ok(AccountRequest::Wallet(WalletReq { address, account_id }))
            }
            (Some(address), None) => Ok(AccountRequest::Account(AccountReq { address })),
            _ => Err(crate::error::service::ServiceError::Parameter(
                "Invalid request: need address".to_string(),
            )),
        }
    }

    type Error = crate::error::service::ServiceError;
}
