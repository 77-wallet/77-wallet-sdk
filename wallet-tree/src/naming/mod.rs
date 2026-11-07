use serde::Serialize;

pub(crate) mod v1;
pub(crate) mod v2;
pub trait NamingStrategy: Send + Sync {
    /// 将元数据编码为文件名
    fn encode(meta: Box<dyn FileMeta>) -> Result<String, crate::Error>;

    /// 从文件名解析元数据
    fn decode(path: &str, filename: &str) -> Result<Box<dyn FileMeta>, crate::Error>;

    /// 验证文件名格式
    fn validate(filename: &str) -> bool;

    /// 策略版本号
    fn version() -> u32;

    /// 生成元数据
    fn generate_filemeta(
        file_type: FileType,
        address: Option<String>,
        account_index_map: Option<&wallet_utils::address::AccountIndexMap>,
        chain_code: Option<String>,
        derivation_path: Option<String>,
    ) -> Result<Box<dyn FileMeta>, crate::Error>;
}

pub trait FileMeta: std::any::Any + Send + Sync {
    fn file_type(&self) -> &FileType;
    fn account_index(&self) -> Option<u32>;
    fn address(&self) -> Option<String>;
    fn chain_code(&self) -> Option<String>;
    fn derivation_path(&self) -> Option<String>;
}

#[derive(Debug, PartialEq, Clone, Serialize)]
pub enum FileType {
    Root,
    PrivateKey,
    Phrase,
    Seed,
    DerivedData,
    DerivedMeta,
    DeprecatedPk,
}
