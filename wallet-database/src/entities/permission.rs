use super::permission_user::PermissionUserEntity;
use chrono::{DateTime, Utc};

#[derive(
    Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow, Clone, Hash, PartialEq, Eq,
)]
#[serde(rename_all = "camelCase")]
pub struct PermissionEntity {
    pub id: String,
    pub name: String,
    pub grantor_addr: String,
    pub types: String,
    pub active_id: i64,
    pub threshold: i64,
    pub member: i64,
    pub chain_code: String,
    pub operations: String,
    pub is_del: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl PermissionEntity {
    pub fn get_id(address: &str, active_id: i64) -> String {
        let id = active_id.to_string();
        let params = vec![address, &id];

        wallet_utils::snowflake::gen_hash_uid(params)
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct PermissionWithUserEntity {
    pub permission: PermissionEntity,
    pub user: Vec<PermissionUserEntity>,
}

impl PermissionWithUserEntity {
    // check whether user has changed include weight
    pub fn user_has_changed(&self, user: &[PermissionUserEntity]) -> bool {
        if self.permission.member != user.len() as i64 {
            return true;
        }

        for old_user in self.user.iter() {
            if !user.iter().any(|u| u.address == old_user.address && u.weight == old_user.weight) {
                return true;
            }
        }
        false
    }

    // all user to string
    pub fn users(&self) -> Vec<String> {
        self.user.iter().map(|u| u.address.clone()).collect()
    }

    pub fn total_weight(&self) -> i32 {
        self.user.iter().map(|u| u.weight as i32).sum::<i32>()
    }

    // 是否有自己的地址
    pub fn has_self_addr(&self) -> bool {
        self.user.iter().any(|u| u.is_self == 1)
    }
}
