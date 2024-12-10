#![feature(let_chains)]
pub mod api;
pub mod error;
pub mod http;
pub mod request;
pub mod response;
pub mod response_vo;
pub use error::Error;
pub use http::{_send_request, send_request};
pub use response_vo::coin::{CoinInfo, TokenPriceInfos};
pub use response_vo::multisig::*;
pub mod consts;
