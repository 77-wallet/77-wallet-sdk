use crate::request::transaction;
use wallet_chain_interact::ton::{
    Cell,
    operations::{BuildInternalMsg, token_transfer::TokenTransferOpt, transfer::TransferOpt},
    provider::Provider,
};
use wallet_types::chain::address::r#type::TonAddressType;
use wallet_utils::unit;

pub async fn build_ext_cell(
    req: &transaction::BaseTransferReq,
    provider: &Provider,
    address_type: TonAddressType,
) -> Result<Cell, crate::error::service::ServiceError> {
    if let Some(token) = req.token_address.to_chain_token_option() {
        let value = unit::convert_to_u256(&req.value, req.decimals)?;
        let arg = TokenTransferOpt::new(&req.from, &req.to, &token, value, req.spend_all)?;

        Ok(arg.build_trans(address_type, provider).await?)
    } else {
        let arg = TransferOpt::new(&req.from, &req.to, &req.value, req.spend_all)?;

        Ok(arg.build_trans(address_type, provider).await?)
    }
}
