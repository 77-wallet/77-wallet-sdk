use crate::{dirs::Dirs, infrastructure};

pub async fn init_log(
    level: Option<&str>,
    app_code: &str,
    dirs: &Dirs,
    sn: &str,
) -> Result<(), crate::error::service::ServiceError> {
    // 修改后的版本
    let format =
        infrastructure::log::format::CustomEventFormat::new(app_code.to_string(), sn.to_string());

    let level = level.unwrap_or("info");

    let path = infrastructure::log::format::LogBasePath(dirs.get_log_dir());
    infrastructure::log::init_logger(format, path, level)?;

    Ok(())
}
