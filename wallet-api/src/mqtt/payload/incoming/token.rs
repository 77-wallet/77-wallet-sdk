use wallet_database::{
    entities::coin::CoinId, factory::RepositoryFactory, repositories::coin::CoinRepoTrait as _,
};
use wallet_types::valueobject::TokenPriceChangeBody;

use crate::{
    response_vo::coin::TokenCurrencies,
    service::{coin::CoinService, exchange_rate::ExchangeRateService},
};

/*
{
    "clientId": "104.2.0.125C00",
    "sn": "104.2.0.125C00",
    "deviceType": "ANDROID",
    "bizType": "TOKEN_PRICE_CHANGE",
    "body": {
        "chainCode": "polygon",
        "code": "chain",
        "defaultToken": false,
        "enable": true,
        "marketValue": 6644971.07,
        "master": false,
        "name": "Chain Games",
        "price": 0.021205427084188898,
        "status": false,
        "tokenAddress": "0xd55fce7cdab84d84f2ef3f99816d765a2a94a509",
        "unit": 18,
    }
}
*/
// biz_type = TOKEN_PRICE_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceChange {
    pub body: TokenPriceChangeBody,
}

impl TokenPriceChange {
    pub(crate) async fn exec(self) -> Result<(), anyhow::Error> {
        let chain_code = &self.body.chain_code;
        let symbol = &self.body.symbol;
        let token_address = &self.body.token_address;
        let price = self.body.price;
        let unit = self.body.unit;
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let coin_id = CoinId {
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
            token_address: token_address.clone(),
        };
        let repo = RepositoryFactory::repo(pool.clone());
        let coin_service = CoinService::new(repo);
        let mut tx = coin_service.repo;
        tx.update_price_unit(&coin_id, &price.to_string(), unit)
            .await?;

        let app_state = crate::app_state::APP_STATE.read().await;
        let currency = app_state.currency();

        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        let exchange_rate = ExchangeRateService::new(repo)
            .detail(Some(currency.to_string()))
            .await?;

        if let Some(exchange_rate) = exchange_rate {
            let res = TokenCurrencies::calculate_token_price_changes(self.body, exchange_rate.rate)
                .await?;
            let data = crate::notify::NotifyEvent::TokenPriceChange(res);
            crate::notify::FrontendNotifyEvent::new(data).send().await?;
        }

        Ok(())
    }
}
