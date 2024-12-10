use crate::{
    dao::coin::Coins,
    entities::coin::{CoinData, CoinEntity, CoinId, SymbolId},
};

#[async_trait::async_trait]
pub trait CoinRepoTrait: super::TransactionTrait {
    async fn upsert_multi_coin(&mut self, coin: Vec<CoinData>) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::upsert_multi_coin, coin)
    }

    async fn coin_list(
        &mut self,
        symbol: Option<&str>,
        chain_code: Option<String>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let symbol = if let Some(symbol) = symbol {
            vec![symbol.to_string()]
        } else {
            Vec::new()
        };
        crate::execute_with_executor!(executor, CoinEntity::list, symbol, chain_code, None)
    }

    async fn coin_list_with_symbols(
        &mut self,
        symbols: Vec<String>,
        chain_code: Option<String>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::list, symbols, chain_code, None)
    }

    async fn default_coin_list(&mut self) -> Result<Vec<CoinEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::list, vec![], None, Some(1))
    }

    async fn get_market_chain_list(&mut self) -> Result<Vec<String>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::chain_code_list,)
    }

    async fn symbol_list(
        &mut self,
        chain_code: Option<String>,
    ) -> Result<Vec<Coins>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::symbol_list, chain_code)
    }

    async fn hot_coin_list_symbol_not_in(
        &mut self,
        chain_codes: &std::collections::HashSet<String>,
        keyword: Option<&str>,
        symbol_list: &std::collections::HashSet<String>,
        page: i64,
        page_size: i64,
    ) -> Result<crate::pagination::Pagination<CoinEntity>, crate::Error> {
        let executor = self.get_db_pool();
        CoinEntity::coin_list_symbol_not_in(
            executor,
            chain_codes,
            keyword,
            symbol_list,
            Some(1),
            None,
            page,
            page_size,
        )
        .await
    }

    async fn update_price_unit(
        &mut self,
        coin_id: &CoinId,
        price: &str,
        unit: Option<u8>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            CoinEntity::update_price_unit,
            coin_id,
            price,
            unit
        )
    }

    async fn drop_coin_just_null_token_address(&mut self) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::drop_coin_just_null_token_address,)
    }

    async fn drop_custom_coin(&mut self, coin_id: &SymbolId) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::drop_custom_coin, coin_id)
    }

    async fn drop_multi_custom_coin(
        &mut self,
        coin_ids: Vec<SymbolId>,
    ) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, CoinEntity::drop_multi_custom_coin, coin_ids)
    }
}
