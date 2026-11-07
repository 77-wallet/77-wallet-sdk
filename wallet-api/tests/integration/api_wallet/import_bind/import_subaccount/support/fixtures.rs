use crate::harness::{derive_uid, next_tag};

use super::super::super::support::SUBACCOUNT_PHRASE;

pub(crate) struct SubaccountImportFixture {
    pub(crate) salt: String,
    pub(crate) wallet_name: String,
    pub(crate) expected_uid: String,
}

impl SubaccountImportFixture {
    pub(crate) fn new(salt_prefix: &str, wallet_name_prefix: &str) -> Self {
        let salt = next_tag(salt_prefix);
        let expected_uid = derive_uid(SUBACCOUNT_PHRASE, &salt);
        Self { salt, wallet_name: next_tag(wallet_name_prefix), expected_uid }
    }
}
