pub(crate) struct ResetRootReq {
    pub(crate) language_code: u8,
    pub(crate) phrase: String,
    pub(crate) salt: String,
    pub(crate) wallet_address: String,
    pub(crate) new_password: String,
    pub(crate) subkey_password: Option<String>,
}
