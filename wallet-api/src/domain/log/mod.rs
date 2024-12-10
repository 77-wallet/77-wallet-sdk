use crate::manager::Context;
use std::time::Duration;
use tokio::time;

pub async fn periodic_log_report(interval: Duration) {
    tokio::spawn(async move {
        let mut interval = time::interval(interval);
        loop {
            interval.tick().await;

            if let Err(e) = upload_log_file().await {
                tracing::error!("upload log file error:{}", e);
            };
        }
    });
}

pub async fn upload_log_file() -> Result<(), crate::ServiceError> {
    let oss_client = crate::manager::Context::get_global_oss_client()?;
    let dirs = Context::get_global_dirs()?;
    let log_dir = &dirs.log_dir.to_string_lossy().to_string();
    let level = wallet_utils::log::get_log_level();
    let src_file_path = format!("{log_dir}/{level}.txt");

    if wallet_utils::file_func::is_file_empty(&src_file_path)? {
        return Ok(());
    };

    let timestamp = sqlx::types::chrono::Utc::now();
    let dst_file_name = format!("sdk:{}.txt", timestamp.format("%Y-%m-%d %H:%M:%S"));
    oss_client
        .upload_local_file(&src_file_path, &dst_file_name)
        .await?;

    let backup_file_path = format!("{log_dir}/{dst_file_name}");
    wallet_utils::file_func::copy_file(&src_file_path, &backup_file_path)?;
    // Clear the log file after successful upload
    wallet_utils::file_func::clear_file(&src_file_path)?;

    Ok(())
}
// Example usage:
// tokio::spawn(periodic_log_report(Duration::from_secs(300)));

#[cfg(test)]
mod tests {

    #[test]
    fn test_generate_dst_file_name() {
        let timestamp = sqlx::types::chrono::Utc::now();
        let dst_file_name = format!("sdk:{}.txt", timestamp.format("%Y-%m-%d %H:%M:%S"));

        // Assert the expected format
        assert_eq!(dst_file_name, "sdk:2023-05-15 10:30:00 UTC.txt");
    }
}
