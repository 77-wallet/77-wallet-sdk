#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    sqlx::Type,
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
)]
#[repr(i64)]
pub enum ApiResourceType {
    Bandwidth = 0,
    Energy = 1,
}

impl ApiResourceType {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_backend_rsc_type(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Bandwidth),
            1 => Some(Self::Energy),
            _ => None,
        }
    }
}
