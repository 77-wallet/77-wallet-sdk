// Nested transfer/observer futures need deeper Send trait resolution.
#![recursion_limit = "256"]
//#![feature(try_trait_v2)]
#![allow(unreachable_code)]
#![allow(clippy::too_many_arguments)]
#![warn(clippy::disallowed_methods)]
#![allow(dead_code)]
#![allow(unused)]

#[allow(clippy::disallowed_methods)]
#[allow(unused)]
mod clippy_config {
    /// 禁止直接使用 tokio::time 方法，必须使用 runtime::time 模块
    /// 防止 replay storm 和其他时间相关问题
    #[clippy::disallow_methods(name("interval"), path("tokio::time"))]
    #[clippy::disallow_methods(name("interval_at"), path("tokio::time"))]
    #[clippy::disallow_methods(name("sleep"), path("tokio::time"))]
    #[clippy::disallow_methods(name("sleep_until"), path("tokio::time"))]
    #[clippy::disallow_methods(name("timeout"), path("tokio::time"))]
    fn _clippy_config() {}
}
pub mod api;
#[cfg(feature = "api-wallet-console")]
pub mod api_wallet_console;
pub mod app_state;
pub mod application;
pub mod batch_transfer;
pub mod config;
pub(crate) mod default_data;
pub mod device_protection;
pub mod domain;
pub mod error;
pub mod infrastructure;

mod context;
#[cfg(any(test, feature = "integration-tests"))]
pub use context::Context;
#[cfg(any(test, feature = "integration-tests"))]
pub use context::api_wallet_backend::ApiWalletBackend;
pub use context::get_context;
mod data;
pub mod dirs;
pub mod manager;
pub mod messaging;

pub mod request;
pub mod response_vo;
pub mod service;
pub mod testkit;

pub mod handles;
pub mod xlog;
