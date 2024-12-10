#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Init database failed")]
    InitDatabase,

    #[error("Database create failed")]
    DatabaseCreateFailed,
    #[error("Database drop failed")]
    DatabaseDropFailed,
    #[error("Database connect failed")]
    DatabaseConnectFailed,

    // conn
    #[error("Get public database connection failed")]
    GetPublicSqliteConnFailed,
    #[error("Get user database connection failed")]
    GetUserSqliteConnFailed,

    // pool
    #[error("Get public database pool failed")]
    GetPublicSqlitePoolFailed,
    #[error("Get user database pool failed")]
    GetUserSqlitePoolFailed,

    // storage
    #[error("Set public storage failed")]
    SetPublicStorageFailed,
    #[error("Get public storage failed")]
    GetPublicStorageFailed,
    #[error("Set user storage failed")]
    SetUserStorageFailed,
    #[error("Get user storage failed")]
    GetUserStorageFailed,

    // action
    #[error("Query data failed")]
    QueryFailed,
    #[error("Insert failed")]
    InsertFailed,
    #[error("Update failed")]
    UpdateFailed,
    #[error("Delete failed")]
    DeleteFailed,

    // migrate
    #[error("Get migrator failed")]
    MigratorGetFailed,
    #[error("Run migration failed")]
    MigrationRunFailed,

    // transaction
    #[error("Transaction not begin")]
    TransactionNotBegin,

    // utils
    #[error("util error: {0}")]
    Util(#[from] wallet_utils::error::Error),

    #[error("RETURNING NONE")]
    ReturningNone,
}

impl DatabaseError {
    pub fn get_status_code(&self) -> u32 {
        match self {
            DatabaseError::Sqlx(_) => 6000,
            DatabaseError::InitDatabase => 6000,
            DatabaseError::DatabaseCreateFailed => 6000,
            DatabaseError::DatabaseDropFailed => 6001,
            DatabaseError::DatabaseConnectFailed => 6002,
            DatabaseError::GetPublicSqliteConnFailed => 6003,
            DatabaseError::GetUserSqliteConnFailed => 6004,
            DatabaseError::GetPublicSqlitePoolFailed => 6005,
            DatabaseError::GetUserSqlitePoolFailed => 6006,
            DatabaseError::SetPublicStorageFailed => 6007,
            DatabaseError::GetPublicStorageFailed => 6008,
            DatabaseError::SetUserStorageFailed => 6009,
            DatabaseError::GetUserStorageFailed => 6010,
            DatabaseError::QueryFailed => 6011,
            DatabaseError::InsertFailed => 6013,
            DatabaseError::UpdateFailed => 6014,
            DatabaseError::DeleteFailed => 6015,
            DatabaseError::MigratorGetFailed => 6016,
            DatabaseError::MigrationRunFailed => 6017,
            DatabaseError::TransactionNotBegin => 6017,
            DatabaseError::Util(_) => 6018,
            DatabaseError::ReturningNone => 6019,
        }
    }
}
