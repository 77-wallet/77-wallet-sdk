use serde::{Deserialize, Serialize};
use sqlx::types::chrono::Utc;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AddressQueryStateEntity {
    pub uid: String,
    pub chain_code: String,
    pub status: AddressQueryStatus,
    pub last_page: i64,
    pub total_remote: i64,
    #[serde(skip_serializing)]
    pub created_at: chrono::DateTime<Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct CreateAddressQueryStateEntity {
    pub uid: String,
    pub chain_code: String,
    pub status: AddressQueryStatus,
    pub last_page: i64,
    pub total_remote: i64,
}

impl CreateAddressQueryStateEntity {
    pub fn new(uid: &str, chain_code: &str, status: AddressQueryStatus) -> Self {
        Self {
            uid: uid.to_string(),
            chain_code: chain_code.to_string(),
            status,
            last_page: -1,
            total_remote: 0,
        }
    }

    pub fn with_last_page(mut self, last_page: i64) -> Self {
        self.last_page = last_page;
        self
    }

    pub fn with_total_remote(mut self, total_remote: i64) -> Self {
        self.total_remote = total_remote;
        self
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    sqlx::Type,
)]
#[repr(u8)]
pub enum AddressQueryStatus {
    Running = 0,
    Done = 1,
    Failed = 2,
}
