#![feature(try_trait_v2, async_closure, let_chains)]
#![allow(unreachable_code)]
#![allow(clippy::too_many_arguments)]
pub mod api;
pub mod app_state;
pub mod config;
pub(crate) mod default_data;
pub mod domain;
mod error;
mod manager;
pub mod mqtt;
pub mod notify;
pub mod request;
mod response;
pub mod response_vo;
pub mod service;
mod system_notification;
pub mod test;

pub use error::{
    business::{
        account::AccountError, announcement::AnnouncementError, assets::AssetsError,
        bill::BillError, chain::ChainError, coin::CoinError, device::DeviceError,
        exchange_rate::ExchangeRate, multisig_account::MultisigAccountError,
        multisig_queue::MultisigQueueError, wallet::WalletError, BusinessError,
    },
    system::SystemError,
    Errors, ServiceError,
};

pub use manager::{Context, WalletManager};
pub use request::assets::GetChain;
pub use request::devices::InitDeviceReq;
pub use test::net_api;
pub use wallet_database::entities::multisig_member::MemberVo;
pub use wallet_transport_backend::request::{
    TokenQueryHistoryPrice, TokenQueryPopularByPageReq, TokenQueryPrice, TokenQueryPriceReq,
};
