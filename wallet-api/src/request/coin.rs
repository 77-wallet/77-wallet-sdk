#[derive(Debug)]
pub struct AddCoinReq {
    pub wallet_address: String,
    pub account_id: u32,
    pub symbol: String,
    pub chain_code: Option<String>,
}
#[derive(Debug)]
pub struct AddMultisigCoinReq {
    pub address: String,
    pub symbol: String,
}
