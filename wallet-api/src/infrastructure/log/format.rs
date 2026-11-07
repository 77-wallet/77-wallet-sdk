use std::path::PathBuf;

use chrono::Local;
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

#[derive(Clone)]
pub struct LogBasePath(pub PathBuf);
impl LogBasePath {
    pub fn log_path(&self) -> PathBuf {
        self.0.join("log.txt")
    }

    pub fn offset_path(&self) -> PathBuf {
        self.0.join("offset.json")
    }

    pub fn back_file_path(&self) -> PathBuf {
        self.0.join("log.1.txt")
    }
}

pub struct CustomEventFormat {
    app_code: String,
    sn: String,
}

impl CustomEventFormat {
    pub fn new(app_code: String, sn: String) -> Self {
        Self { app_code, sn }
    }
}

impl<S, N> FormatEvent<S, N> for CustomEventFormat
where
    S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();

        // 时间
        write!(writer, "{} ", Local::now().format("%Y-%m-%d %H:%M:%S%.3f"))?;

        // app_code
        write!(writer, "{} ", self.app_code)?;

        // 日志级别
        write!(writer, "{} ", meta.level())?;

        // 操作系统信息
        write!(writer, "{} ", wallet_utils::system_info::get_os_info())?;

        // 日志目标
        write!(writer, "{} ", meta.target())?;

        // ip地址
        write!(writer, "127.0.0.1 ")?;

        // 设备sn
        write!(writer, "{} ", self.sn)?;

        // 事件字段
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}
