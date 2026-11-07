#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("This mnemonic phrase has been imported into the normal wallet system")]
    MnemonicAlreadyImportedIntoNormalWalletSystem,
    #[error("Api Wallet not exist, please check your input")]
    NotFound,
    #[error("Import is not supported for this wallet type")]
    ImportNotSupportedForThisWalletType,
    #[error("Api wallet already imported")]
    AlreadyImported,
    // APPID已绑定，请勿重复操作
    #[error("AppId already binded, do not repeat operation")]
    AppIdAlreadyBinded,
    // 该出款钱包未在该appId下使用过
    #[error("This withdrawal wallet has not been used under this appId")]
    WithdrawalWalletNotUsed,
    // 子账户钱包未绑定
    #[error("The sub account wallet is unbound")]
    SubAccountWalletNotBound,
    #[error("Api Wallet already exists")]
    AlreadyExist,
    // 子账户钱包未绑定出款钱包地址
    #[error("The sub account wallet is unbound withdrawal wallet address")]
    SubAccountWalletNotBoundWithdrawalWalletAddress,
    // 子账户钱包导入未完成
    #[error("The sub account wallet import is not completed")]
    SubAccountWalletImportNotCompleted,
}

impl WalletError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            WalletError::MnemonicAlreadyImportedIntoNormalWalletSystem => 20000,
            WalletError::NotFound => 20001,
            WalletError::ImportNotSupportedForThisWalletType => 20002,
            WalletError::AlreadyImported => 20003,
            WalletError::WithdrawalWalletNotUsed => 20004,
            WalletError::AppIdAlreadyBinded => 20005,
            WalletError::SubAccountWalletNotBound => 20006,
            WalletError::AlreadyExist => 20007,
            WalletError::SubAccountWalletNotBoundWithdrawalWalletAddress => 20008,
            WalletError::SubAccountWalletImportNotCompleted => 20009,
        }
    }
}
