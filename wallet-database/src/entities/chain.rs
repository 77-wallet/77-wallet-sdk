use crate::entities::api_chain::NodeBindType;

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ChainEntity {
    pub name: String,
    pub chain_code: String,
    pub main_symbol: String,
    pub node_id: Option<String>,
    // #[sqlx(type_name = "TEXT")]
    pub protocols: StringList,
    pub node_bind_type: NodeBindType,
    pub status: u8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct StringList(pub Vec<String>);

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for StringList {
    fn decode(
        value: <sqlx::Sqlite as sqlx::Database>::ValueRef<'r>,
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

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ChainCreateVo {
    pub name: String,
    pub chain_code: String,
    pub protocols: Vec<String>,
    pub node_bind_type: NodeBindType,
    pub status: u8,
    pub main_symbol: String,
}

impl ChainCreateVo {
    pub fn new(
        name: &str,
        chain_code: &str,
        protocols: &[String],
        node_bind_type: NodeBindType,
        main_symbol: &str,
    ) -> ChainCreateVo {
        Self {
            name: name.to_string(),
            chain_code: chain_code.to_string(),
            protocols: protocols.to_vec(),
            node_bind_type,
            status: 1,
            main_symbol: main_symbol.to_string(),
        }
    }

    pub fn with_status(mut self, status: u8) -> Self {
        self.status = status;
        self
    }
}

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
impl ChainWithNode {
    pub fn get_network(&self) -> &str {
        if self.network.is_empty() { "mainnet" } else { &self.network }
    }
}

#[async_trait::async_trait]
pub trait ChainLike {
    fn chain_code(&self) -> &str;
    fn status(&self) -> u8;
    fn node_id(&self) -> Option<&String>;

    async fn set_node(
        pool: &crate::CoreDbPool,
        chain_code: &str,
        node_id: &str,
        bind_type: super::api_chain::NodeBindType,
    ) -> Result<(), crate::Error>;
}

#[async_trait::async_trait]
impl ChainLike for ChainEntity {
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn status(&self) -> u8 {
        self.status
    }
    fn node_id(&self) -> Option<&String> {
        self.node_id.as_ref()
    }

    async fn set_node(
        pool: &crate::CoreDbPool,
        chain_code: &str,
        node_id: &str,
        bind_type: super::api_chain::NodeBindType,
    ) -> Result<(), crate::Error> {
        use crate::repositories::chain::ChainRepo;
        ChainRepo::set_chain_node_with_type(pool, chain_code, node_id, bind_type).await?;
        Ok(())
    }
}
