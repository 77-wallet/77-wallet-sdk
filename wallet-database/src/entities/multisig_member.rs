use super::multisig_signatures::MultisigSignatureEntity;
use sqlx::types::chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};
use wallet_types::valueobject::AddressPubkey;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MultisigMemberEntity {
    pub account_id: String,
    pub address: String,
    pub name: String,
    // 确认状态(0,未确认,1,已确认)
    pub confirmed: i8,
    // self(0,不是,1,是)
    pub is_self: i8,
    pub pubkey: String,
    pub uid: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow, Clone)]
pub struct MultisigMemberEntities(pub Vec<MultisigMemberEntity>);

impl Deref for MultisigMemberEntities {
    type Target = Vec<MultisigMemberEntity>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MultisigMemberEntities {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl MultisigMemberEntities {
    pub fn get_owner_pubkey(&self) -> Vec<AddressPubkey> {
        let mut owners: Vec<AddressPubkey> = self
            .0
            .iter()
            .map(|e| AddressPubkey {
                address: e.address.clone(),
                pubkey: e.pubkey.clone(),
            })
            .collect();
        owners.sort_by(|a, b| a.address.cmp(&b.address));
        owners
    }

    pub fn get_owner_str_vec(&self) -> Vec<String> {
        let mut owners: Vec<String> = self.0.iter().map(|e| e.address.clone()).collect();
        owners.sort();
        owners
    }

    pub fn sign_order(&self, signed: &[MultisigSignatureEntity]) -> Vec<String> {
        // bitcoin p2tr-sh 需要按照地址倒叙
        let mut c = self.0.clone();
        c.sort_by(|a, b| b.address.cmp(&a.address));

        let mut siged_map = HashMap::new();
        for sig in signed.iter() {
            siged_map.insert(&sig.address, &sig.signature);
        }
        let mut res = vec![];
        for m in c.iter() {
            let sign = siged_map.get(&m.address);
            match sign {
                Some(sign) => res.push(sign.to_string()),
                None => res.push(String::new()),
            }
        }
        res
    }

    pub fn all_confirmed(&self) -> bool {
        self.0.iter().all(|item| item.confirmed == 1)
    }

    pub fn prioritize_by_address(&mut self, target_address: &str) {
        self.0.sort_by(|a, b| {
            if a.address == target_address {
                std::cmp::Ordering::Less
            } else if b.address == target_address {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
    }
}

#[derive(Debug)]
pub struct NewMemberEntity {
    pub account_id: String,
    pub name: String,
    pub address: String,
    pub pubkey: String,
    pub confirmed: i8,
    pub is_self: i8,
    pub uid: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MemberVo {
    // 参与方名称
    pub name: String,
    // 参与方地址
    pub address: String,
    pub pubkey: String,
    // 确认状态
    pub confirmed: i8,
    pub uid: String,
}

impl MemberVo {
    pub fn new(name: String, address: String) -> Self {
        Self {
            name,
            address,
            pubkey: "".to_string(),
            uid: "".to_string(),
            confirmed: 0,
        }
    }
}
