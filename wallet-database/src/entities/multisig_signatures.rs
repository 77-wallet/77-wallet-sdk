use super::permission_user::PermissionUserEntity;
use sqlx::types::chrono::{DateTime, Utc};

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow, Clone)]
pub struct MultisigSignatureEntity {
    pub id: i64,
    pub queue_id: String,
    pub address: String,
    pub signature: String,
    pub status: i8,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MultisigSignatureEntities(pub Vec<MultisigSignatureEntity>);

impl MultisigSignatureEntities {
    pub fn get_order_sign_str(&self) -> Vec<String> {
        let mut list = self.0.clone();
        list.sort_by(|a, b| a.address.cmp(&b.address));

        list.iter().map(|s| s.signature.clone()).collect::<Vec<String>>()
    }

    pub fn contains_address(&self, address: &str) -> bool {
        self.0
            .iter()
            .any(|s| s.address == address && s.status == MultisigSignatureStatus::Approved.to_i8())
    }

    pub fn need_signed_num(&self, threshold: usize) -> usize {
        threshold.saturating_sub(self.0.len())
    }
}

#[derive(Clone, Debug, Copy, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(u8)]
pub enum MultisigSignatureStatus {
    UnSigned = 0, // 未签
    Approved = 1, // 同意
    Rejected = 2, // 拒绝
}
impl TryFrom<i32> for MultisigSignatureStatus {
    type Error = crate::Error;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MultisigSignatureStatus::UnSigned),
            1 => Ok(MultisigSignatureStatus::Approved),
            2 => Ok(MultisigSignatureStatus::Rejected),
            _ => Err(crate::Error::Other(format!("invalid multisig signature status {}", value))),
        }
    }
}

impl MultisigSignatureStatus {
    pub fn to_i8(&self) -> i8 {
        match self {
            MultisigSignatureStatus::UnSigned => 0,
            MultisigSignatureStatus::Approved => 1,
            MultisigSignatureStatus::Rejected => 2,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct NewSignatureEntity {
    pub queue_id: String,
    pub address: String,
    pub signature: String,
    pub status: MultisigSignatureStatus,
    // 如果是权限的签名,有值，其他的默认None
    pub weight: Option<i32>,
}
impl NewSignatureEntity {
    pub fn new(
        queue_id: &str,
        address: &str,
        signature: &str,
        status: MultisigSignatureStatus,
        weight: Option<i32>,
    ) -> Self {
        NewSignatureEntity {
            queue_id: queue_id.to_string(),
            address: address.to_string(),
            signature: signature.to_string(),
            status,
            weight,
        }
    }

    pub fn new_approve(
        queue_id: &str,
        address: &str,
        signature: String,
        weight: Option<i32>,
    ) -> Self {
        NewSignatureEntity {
            queue_id: queue_id.to_string(),
            address: address.to_string(),
            signature,
            status: MultisigSignatureStatus::Approved,
            weight,
        }
    }

    pub fn new_no_queue_id(address: &str, signature: &str, weight: Option<i32>) -> Self {
        NewSignatureEntity {
            queue_id: "".to_string(),
            address: address.to_string(),
            signature: signature.to_string(),
            status: MultisigSignatureStatus::Approved,
            weight,
        }
    }
}

impl From<(&PermissionUserEntity, &str)> for NewSignatureEntity {
    fn from(value: (&PermissionUserEntity, &str)) -> Self {
        NewSignatureEntity {
            queue_id: value.1.to_string(),
            address: value.0.address.clone(),
            signature: String::new(),
            status: MultisigSignatureStatus::UnSigned,
            weight: Some(value.0.weight as i32),
        }
    }
}

// 这里缺失weight字段
impl TryFrom<MultisigSignatureEntity> for NewSignatureEntity {
    type Error = crate::Error;
    fn try_from(value: MultisigSignatureEntity) -> Result<Self, Self::Error> {
        Ok(NewSignatureEntity {
            queue_id: value.queue_id,
            address: value.address,
            signature: value.signature,
            status: MultisigSignatureStatus::try_from(value.status as i32)?,
            weight: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MultisigSignatureEntities, MultisigSignatureEntity};
    use sqlx::types::chrono::Utc;

    fn signature(id: i64) -> MultisigSignatureEntity {
        MultisigSignatureEntity {
            id,
            queue_id: "queue".to_string(),
            address: format!("signer-{id}"),
            signature: "signature".to_string(),
            status: 1,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn need_signed_num_handles_threshold_boundaries() {
        let signatures = MultisigSignatureEntities(vec![signature(1), signature(2), signature(3)]);

        assert_eq!(signatures.need_signed_num(5), 2);
        assert_eq!(signatures.need_signed_num(3), 0);
        assert_eq!(signatures.need_signed_num(2), 0);
    }
}
