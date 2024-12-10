use wallet_database::{
    entities::exchange_rate::ExchangeRateEntity,
    repositories::{exchange_rate::ExchangeRateRepoTrait, ResourcesRepo, TransactionTrait as _},
};

use crate::domain::coin::CoinDomain;

pub struct ExchangeRateService {
    pub repo: ResourcesRepo,
    pub coin_domain: CoinDomain,
    // keystore: wallet_keystore::Keystore
}

impl ExchangeRateService {
    pub fn new(repo: ResourcesRepo) -> Self {
        Self {
            repo,
            coin_domain: CoinDomain::new(),
        }
    }

    pub async fn upsert(
        self,
        target_currency: &str,
        name: &str,
        price: f64,
    ) -> Result<Vec<ExchangeRateEntity>, crate::ServiceError> {
        let mut tx = self.repo;
        let res = tx.upsert(target_currency, name, price).await?;
        Ok(res)
    }

    pub async fn detail(
        self,
        target_currency: Option<String>,
    ) -> Result<Option<ExchangeRateEntity>, crate::ServiceError> {
        let mut tx = self.repo;
        let res = tx.detail(target_currency).await?;
        Ok(res)
    }

    pub async fn init(
        self,
        rates: wallet_transport_backend::response_vo::coin::TokenRates,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;

        for rate in rates.list.into_iter() {
            tx.upsert(&rate.target_currency, &rate.name, rate.rate)
                .await?;
        }
        tx.commit_transaction().await?;
        Ok(())
    }
    // pub async fn init(self) -> Result<(), crate::ServiceError> {
    //     let mut tx = self.repo;
    //     let coins = tx.coin_list(None, None).await?;
    //     let mut exchange_rate_ids = std::collections::HashMap::new();
    //     for coin in &coins {
    //         let token_currency =
    //             crate::service::get_current_coin_unit_price_option(&coin.symbol, &coin.chain_code)
    //                 .await?;
    //         if let Some(token_currency) = token_currency {
    //             let price = token_currency.price;
    //             let exchange_rate_id = ExchangeRateId {
    //                 currency: coin.symbol.clone(),
    //                 chain_code: coin.chain_code.clone(),
    //                 symbol: coin.symbol.clone(),
    //             };
    //             exchange_rate_ids.insert(exchange_rate_id, price);
    //         }
    //     }

    //     for (exchange_rate_id, price) in exchange_rate_ids.into_iter() {
    //         tx.upsert(exchange_rate_id, price).await?;
    //     }

    //     Ok(())
    // }
}
