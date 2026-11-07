// use wallet_database::{
//     entities::assets::AssetsEntity,
//     repositories::{
//         account::AccountRepoTrait, assets::AssetsRepoTrait, coin::CoinRepoTrait,
//         exchange_rate::ExchangeRateRepoTrait,
//     },
//     sqlite::logic::multisig_account::MultisigAccountDto,
// };

// use crate::{
//     global_context::GlobalContext as _,
//     response_vo::coin::TokenCurrencies,
//     service::{asset::AddressChainCode, Service},
// };

// pub struct CoinDomain<T> {
//     phantom: std::marker::PhantomData<T>,
// }

// impl<T: CoinRepoTrait + ExchangeRateRepoTrait> CoinDomain<T> {
//     pub fn new() -> Self {
//         Self {
//             phantom: std::marker::PhantomData,
//         }
//     }

//     /// 查询代币汇率
//     // pub async fn get_token_currencies(
//     //     &self,
//     //     repo: &mut T,
//     // ) -> Result<TokenCurrencies, crate::ServiceError> {
//     //     let mut token_currencies = std::collections::HashMap::new();
//     //     let coins = repo.coin_list(None, None).await?;
//     //     for coin in &coins {
//     //         let token_currency =
//     //             crate::service::get_current_coin_unit_price_option(&coin.symbol, &coin.chain_code)
//     //                 .await?;
//     //         if let Some(token_currency) = token_currency {
//     //             token_currencies.insert(token_currency.code.clone(), token_currency);
//     //         }
//     //     }

//     //     Ok(TokenCurrencies(token_currencies))
//     // }

// }
