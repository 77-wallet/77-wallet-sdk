pub struct AddressBookDomain;

impl AddressBookDomain {
    pub async fn check_address(
        address: String,
        chain_code: String,
    ) -> Result<(), crate::error::service::ServiceError> {
        let net = wallet_types::chain::network::NetworkKind::Mainnet;

        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code.as_ref())?;

        // check address format is right
        crate::domain::chain::check_address(&address, chain, net)?;

        Ok(())
    }
}
