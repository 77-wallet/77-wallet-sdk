#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionChangeFrontend {
    // 授权方的地址
    pub grantor_addr: String,
    // 操作类型 (new,update,delete)
    pub types: String,
    // 对应的权限码
    pub operations: Vec<i8>,
}

impl PermissionChangeFrontend {
    pub fn new(grantor_addr: &str, types: &str, operations: Vec<i8>) -> Self {
        Self { grantor_addr: grantor_addr.to_owned(), types: types.to_owned(), operations }
    }
}
