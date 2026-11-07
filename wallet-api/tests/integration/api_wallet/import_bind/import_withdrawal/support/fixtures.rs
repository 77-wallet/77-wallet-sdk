use crate::harness::{derive_uid, next_tag};

use super::super::super::support::WITHDRAWAL_PHRASE;

pub(crate) struct RechargeWalletFixture {
    pub(crate) uid: String,
    pub(crate) address: String,
}

pub(crate) struct WithdrawalImportFixture {
    pub(crate) salt: String,
    pub(crate) wallet_name: String,
    pub(crate) binding_address: String,
    pub(crate) expected_uid: String,
}

impl WithdrawalImportFixture {
    pub(crate) fn new(salt_prefix: &str, wallet_name_prefix: &str, binding_address: &str) -> Self {
        let salt = next_tag(salt_prefix);
        let expected_uid = derive_uid(WITHDRAWAL_PHRASE, &salt);
        Self {
            salt,
            wallet_name: next_tag(wallet_name_prefix),
            binding_address: binding_address.to_string(),
            expected_uid,
        }
    }
}
