use wallet_api::request::api_wallet::{trans::ApiBaseTransferReq, transfer::ApiTransferExReq};

use crate::harness::SMOKE_WALLET_PASSWORD;

pub(crate) const BNB_TO_ADDR: &str = "0x998522f928A37837Fa8d6743713170243b95f98a";

pub(crate) fn make_transfer_req(from: &str, to: &str) -> ApiTransferExReq {
    let mut base = ApiBaseTransferReq::new(from, to, "0.0000001", "bnb");
    base.with_token(None, 18, "BNB");
    ApiTransferExReq {
        base,
        password: SMOKE_WALLET_PASSWORD.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    }
}

pub(crate) fn native_symbol(chain: &str) -> &'static str {
    match chain {
        "bnb" => "BNB",
        "polygon" => "POL",
        "avalanche" => "AVAX",
        "fantom" => "FTM",
        _ => "ETH",
    }
}

pub(crate) fn make_evm_transfer_req(from: &str, chain: &str) -> ApiTransferExReq {
    let mut req = make_transfer_req(from, BNB_TO_ADDR);
    req.base.chain_code = chain.to_string();
    req.base.symbol = native_symbol(chain).to_string();
    req
}

pub(crate) fn evm_chain_codes() -> Vec<String> {
    wallet_types::chain::chain::ChainCodes::try_from(wallet_types::constant::chain_type::ETH_TYPE)
        .unwrap()
        .0
        .into_iter()
        .map(|chain| chain.to_string())
        .collect()
}
