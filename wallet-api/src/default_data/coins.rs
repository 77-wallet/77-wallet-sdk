#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultCoin {
    pub(crate) name: String,
    pub(crate) symbol: String,
    pub(crate) chain_code: String,
    pub(crate) decimals: u8,
    pub(crate) token_address: Option<String>,
    pub(crate) protocol: Option<String>,
    // pub(crate) default: bool,
}
static INIT_COINS_INFO: once_cell::sync::Lazy<once_cell::sync::OnceCell<Vec<DefaultCoin>>> =
    once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);

pub(crate) fn init_default_coins_list() -> Result<&'static Vec<DefaultCoin>, crate::ServiceError> {
    INIT_COINS_INFO.get_or_try_init(|| {
        // let mut res = std::collections::HashMap::new();
        let coin_json = include_str!("../../data/default_chain_list/coins.json");
        let coin_json_data: Vec<DefaultCoin> = wallet_utils::serde_func::serde_from_str(coin_json)?;
        Ok(coin_json_data)
    })
}

impl From<DefaultCoin> for wallet_transport_backend::CoinInfo {
    fn from(value: DefaultCoin) -> Self {
        Self {
            symbol: Some(value.symbol),
            name: Some(value.name),
            chain_code: Some(value.chain_code),
            token_address: value.token_address,
            protocol: value.protocol,
            id: Default::default(),
            decimals: Some(value.decimals),
        }
    }
}
