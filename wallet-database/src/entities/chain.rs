#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ChainEntity {
    pub name: String,
    pub chain_code: String,
    pub main_symbol: String,
    pub node_id: String,
    // #[sqlx(type_name = "TEXT")]
    pub protocols: StringList,
    pub status: u8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct StringList(pub Vec<String>);

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for StringList {
    fn decode(
        value: <sqlx::Sqlite as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;

        // now you can parse this into your type (assuming there is a `FromStr`)
        // let value = value.as_str()?;
        let list: Vec<String> = serde_json::from_str(value)?;
        Ok(StringList(list))
    }
}

impl sqlx::Type<sqlx::Sqlite> for StringList {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct ChainCreateVo {
    pub name: String,
    pub chain_code: String,
    pub node_id: String,
    pub protocols: Vec<String>,
    pub status: u8,
    pub main_symbol: String,
}

impl ChainCreateVo {
    pub fn new(
        name: &str,
        chain_code: &str,
        node_id: &str,
        protocols: &[String],
        main_symbol: &str,
    ) -> ChainCreateVo {
        Self {
            name: name.to_string(),
            chain_code: chain_code.to_string(),
            node_id: node_id.to_string(),
            protocols: protocols.to_vec(),
            status: 1,
            main_symbol: main_symbol.to_string(),
        }
    }
}

// #[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
// pub struct ChainCreateV2 {
//     pub name: String,
//     pub chain_code: String,
//     pub node_id: String,
//     pub protocols: Vec<String>,
//     pub main_symbol: String,
// }

// impl ChainCreateV2 {
//     pub fn new(
//         name: &str,
//         chain_code: &str,
//         node_id: &str,
//         protocols: &[String],
//         main_symbol: &str,
//     ) -> ChainCreateV2 {
//         Self {
//             name: name.to_string(),
//             chain_code: chain_code.to_string(),
//             node_id: node_id.to_string(),
//             protocols: protocols.to_vec(),
//             main_symbol: main_symbol.to_string(),
//         }
//     }
// }

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ChainWithNode {
    pub name: String,
    pub chain_code: String,
    pub main_symbol: String,
    pub node_id: String,
    pub node_name: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub http_url: String,
    pub network: String,
    pub status: u8,
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}
