/// 0: initialization, 1: backend_api, 2: mqtt
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
#[repr(i64)]
pub(crate) enum TaskType {
    Initialization = 0,
    BackendApi = 1,
    Mqtt = 2,
    Common = 3,
}

impl sqlx::FromRow<'_, sqlx::sqlite::SqliteRow> for TaskType {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        use sqlx::Row as _;
        let value = row.try_get::<i64, _>("type")?;
        Self::try_from(value).map_err(|_| sqlx::Error::RowNotFound)
    }
}

impl sqlx::Encode<'_, sqlx::sqlite::Sqlite> for TaskType {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::sqlite::Sqlite as sqlx::Database>::ArgumentBuffer<'_>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let value = *self as i64;
        buf.push(sqlx::sqlite::SqliteArgumentValue::Int64(value));
        Ok(sqlx::encode::IsNull::No)
    }
}

impl sqlx::Decode<'_, sqlx::sqlite::Sqlite> for TaskType {
    fn decode(
        value: <sqlx::sqlite::Sqlite as sqlx::Database>::ValueRef<'_>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <i64 as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        Self::try_from(value)
    }
}

impl TryFrom<i64> for TaskType {
    type Error = sqlx::error::BoxDynError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TaskType::Initialization),
            1 => Ok(TaskType::BackendApi),
            2 => Ok(TaskType::Mqtt),
            3 => Ok(TaskType::Common),
            _ => Err(Box::new(sqlx::Error::ColumnNotFound("Invalid TaskType value".into()))),
        }
    }
}
