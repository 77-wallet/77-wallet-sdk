pub mod format;
mod offset_tracker;
mod rotator;
pub mod upload_log;

use crate::infrastructure::log::format::{CustomEventFormat, LogBasePath};
use rotator::SizeRotatingWriter;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, filter::LevelFilter, fmt, layer::SubscriberExt,
};

fn build_stdout_filter(log_level: &str) -> EnvFilter {
    let stdout_level = log_level
        .parse::<LevelFilter>()
        .ok()
        .map(|level| match level {
            LevelFilter::TRACE | LevelFilter::DEBUG | LevelFilter::INFO => LevelFilter::INFO,
            LevelFilter::WARN => LevelFilter::WARN,
            LevelFilter::ERROR => LevelFilter::ERROR,
            _ => LevelFilter::INFO,
        })
        .unwrap_or(LevelFilter::INFO);

    let stdout_level = match stdout_level {
        LevelFilter::TRACE | LevelFilter::DEBUG | LevelFilter::INFO => "info",
        LevelFilter::WARN => "warn",
        LevelFilter::ERROR => "error",
        _ => "info",
    };

    EnvFilter::new(stdout_level)
}

// 初始化日志。
pub fn init_logger(
    format: CustomEventFormat,
    path: LogBasePath,
    log_level: &str,
) -> Result<(), crate::error::service::ServiceError> {
    let writer = SizeRotatingWriter::new(path.log_path())?;
    let (non_blocking, guard) = tracing_appender::non_blocking(writer);

    let file_filter = EnvFilter::new(log_level);
    let stdout_filter = build_stdout_filter(log_level);

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        // .with_file(true)         // ✅ 显示文件名
        .with_line_number(true)
        .event_format(format)
        .with_filter(file_filter.clone());

    // 构建总的 subscriber
    #[cfg(target_os = "android")]
    {
        let android_layer =
            tracing_android::layer("plugin").unwrap().with_filter(file_filter.clone());
        let subscriber = Registry::default().with(android_layer).with(file_layer);

        // Tests may initialize logging more than once; ignore the second init.
        #[cfg(test)]
        let _ = tracing::subscriber::set_global_default(subscriber);
        #[cfg(not(test))]
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global tracing subscriber");
    }

    #[cfg(target_os = "ios")]
    {
        let subscriber = Registry::default().with(file_layer);
        // Tests may initialize logging more than once; ignore the second init.
        #[cfg(test)]
        let _ = tracing::subscriber::set_global_default(subscriber);
        #[cfg(not(test))]
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global tracing subscriber");
    }

    #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
    {
        let stdout_layer = fmt::layer()
            .with_writer(std::io::stdout) // <-- 新增
            .with_ansi(true)
            // .with_file(true)         // ✅ 显示文件名
            .with_line_number(true)
            .with_filter(stdout_filter);

        let subscriber = Registry::default().with(file_layer).with(stdout_layer);

        // Tests may initialize logging more than once; ignore the second init.
        #[cfg(test)]
        let _ = tracing::subscriber::set_global_default(subscriber);
        #[cfg(not(test))]
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global tracing subscriber");
    }

    std::mem::forget(guard);
    Ok(())
}
