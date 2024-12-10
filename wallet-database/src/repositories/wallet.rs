use crate::entities::wallet::WalletEntity;

#[async_trait::async_trait]
pub trait WalletRepoTrait: super::TransactionTrait {
    async fn upsert_wallet(
        &mut self,
        address: &str,
        uid: &str,
        name: &str,
    ) -> Result<WalletEntity, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::upsert, address, uid, name, 1)
    }

    async fn detail_all_status(
        &mut self,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::detail_all_status, address)
    }

    async fn update_wallet_update_at(
        &mut self,
        wallet_address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            WalletEntity::update_wallet_update_at,
            wallet_address
        )
    }

    async fn wallet_init(&mut self, uid: &str) -> Result<WalletEntity, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::init, uid)
    }

    async fn edit_wallet_name(
        &mut self,
        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            WalletEntity::edit_wallet_name,
            wallet_address,
            name
        )
    }

    async fn detail(&mut self, address: &str) -> Result<Option<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::detail, address)
    }

    async fn wallet_latest(&mut self) -> Result<Option<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::wallet_latest,)
    }

    async fn uid_list(&mut self) -> Result<Vec<(String,)>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::uid_list,)
    }

    async fn wallet_detail_by_name(
        &mut self,
        name: Option<String>,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::wallet_detail_by_wallet_name, name)
    }

    async fn wallet_detail_by_address(
        &mut self,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            WalletEntity::wallet_detail_by_wallet_address,
            address
        )
    }

    async fn wallet_list(&mut self) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::list,)
    }

    async fn reset(&mut self, wallet_address: &str) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::reset_wallet, wallet_address)
    }

    async fn reset_all_wallet(&mut self) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::reset_all_wallet,)
    }

    async fn restart(
        &mut self,
        wallet_addresses: &[&str],
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::restart_wallet, wallet_addresses)
    }

    async fn physical_delete(
        &mut self,
        wallet_address: &[&str],
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::delete_wallet, wallet_address)
    }

    async fn physical_delete_all(&mut self) -> Result<Vec<WalletEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, WalletEntity::delete_all_wallet,)
    }
}
