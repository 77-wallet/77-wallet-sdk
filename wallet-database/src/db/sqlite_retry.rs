use std::{future::Future, time::Duration};

const MAX_LOCK_RETRIES: u32 = 2;
const INITIAL_BACKOFF_MS: u64 = 30;

pub async fn with_sqlite_locked_retry<T, F, Fut>(mut op: F) -> Result<T, crate::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, crate::Error>>,
{
    let mut retry_count = 0;

    loop {
        match op().await {
            Ok(v) => {
                if retry_count > 0 {
                    tracing::info!(
                        metric = "sqlite_locked_retry_count",
                        value = %retry_count,
                        "sqlite lock retry recovered"
                    );
                }
                return Ok(v);
            }
            Err(err) if is_sqlite_locked(&err) && retry_count < MAX_LOCK_RETRIES => {
                let delay_ms = INITIAL_BACKOFF_MS * (1_u64 << retry_count);
                retry_count += 1;
                tracing::warn!(
                    retry_count = %retry_count,
                    max_retries = %MAX_LOCK_RETRIES,
                    delay_ms = %delay_ms,
                    "sqlite lock detected, retrying operation"
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            Err(err) => {
                if retry_count > 0 {
                    tracing::warn!(
                        metric = "sqlite_locked_retry_count",
                        value = %retry_count,
                        "sqlite lock retry exhausted"
                    );
                }
                return Err(err);
            }
        }
    }
}

fn is_sqlite_locked(err: &crate::Error) -> bool {
    if let crate::Error::Database(crate::DatabaseError::Sqlx(sqlx::Error::Database(db_err))) = err {
        if db_err.code().as_deref() == Some("5") {
            return true;
        }
        return db_err.message().to_ascii_lowercase().contains("database is locked");
    }
    false
}
