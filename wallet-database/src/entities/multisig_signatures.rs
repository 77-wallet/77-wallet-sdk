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

        list.iter()
            .map(|s| s.signature.clone())
            .collect::<Vec<String>>()
    }

    pub fn contains_address(&self, address: &str) -> bool {
        self.0
            .iter()
            .any(|s| s.address == address && s.status == MultisigSignatureStatus::Approved.to_i8())
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
            _ => Err(crate::Error::Other(format!(
                "invalid multisig signature status {}",
                value
            ))),
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
}
impl NewSignatureEntity {
    pub fn new(
        queue_id: &str,
        address: &str,
        signature: &str,
        status: MultisigSignatureStatus,
    ) -> Self {
        NewSignatureEntity {
            queue_id: queue_id.to_string(),
            address: address.to_string(),
            signature: signature.to_string(),
            status,
        }
    }

    pub fn new_no_queue_id(address: &str, signature: &str) -> Self {
        NewSignatureEntity {
            queue_id: "".to_string(),
            address: address.to_string(),
            signature: signature.to_string(),
            status: MultisigSignatureStatus::Approved,
        }
    }
}

impl TryFrom<MultisigSignatureEntity> for NewSignatureEntity {
    type Error = crate::Error;
    fn try_from(value: MultisigSignatureEntity) -> Result<Self, Self::Error> {
        Ok(NewSignatureEntity {
            queue_id: value.queue_id,
            address: value.address,
            signature: value.signature,
            status: MultisigSignatureStatus::try_from(value.status as i32)?,
        })
    }
}
