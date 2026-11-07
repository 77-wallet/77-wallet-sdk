use std::collections::HashSet;

use wallet_chain_interact::tron::operations::{
    multisig::{Keys, Permission},
    permissions::PermissionTypes,
};

pub struct PermissionReq {
    pub grantor_addr: String,
    pub name: String,
    pub active_id: Option<i8>,
    pub threshold: i8,
    pub operations: Vec<i8>,
    pub keys: Vec<KeysReq>,
}

impl PermissionReq {
    pub const NEW: &'static str = "new";
    pub const UPDATE: &'static str = "update";
    pub const DELETE: &'static str = "delete";

    pub fn check_threshold(&self) -> Result<(), crate::error::business::BusinessError> {
        let weight = self.keys.iter().map(|k| k.weight).sum::<i32>();
        if weight < self.threshold as i32 {
            return Err(crate::error::business::BusinessError::Permission(
                crate::error::business::permission::PermissionError::WeightLessThreshold,
            ))?;
        }
        Ok(())
    }

    pub fn users(&self) -> Vec<String> {
        self.keys
            .iter()
            .map(|k| k.address.clone())
            .collect::<HashSet<String>>()
            .into_iter()
            .collect()
    }
}

impl TryFrom<&PermissionReq> for Permission {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &PermissionReq) -> Result<Self, Self::Error> {
        let operations = PermissionTypes::from_i8(value.operations.clone())?;

        let mut keys = vec![];
        for item in value.keys.iter() {
            let key = Keys::new(&item.address, item.weight)?;
            keys.push(key);
        }

        Ok(Permission::new_actives(
            value.name.clone(),
            operations.to_hex(),
            value.threshold as u8,
            keys,
        ))
    }
}

pub struct KeysReq {
    pub address: String,
    pub weight: i32,
}
