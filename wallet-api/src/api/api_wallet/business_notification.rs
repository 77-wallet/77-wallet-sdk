use crate::{
    api::ReturnType, manager::WalletManager,
    service::api_wallet::business_notification::ApiBusinessNotificationService,
};
use wallet_database::pagination::Pagination;

use crate::response_vo::api_wallet::business_notification::{
    ApiBusinessNotificationDetail, ApiBusinessNotificationSummary, summarize_page,
};

impl WalletManager {
    pub async fn get_api_business_notification_detail(
        &self,
        id: &str,
    ) -> ReturnType<Option<ApiBusinessNotificationDetail>> {
        ApiBusinessNotificationService::new(self.ctx).detail_for_withdrawal_wallets(id).await
    }

    /// Unresolved address-balance notifications across active API withdrawal wallets.
    pub async fn get_unresolved_api_address_balance_notifications(
        &self,
    ) -> ReturnType<Vec<ApiBusinessNotificationSummary>> {
        ApiBusinessNotificationService::new(self.ctx)
            .unresolved_address_balance_notifications()
            .await
            .map(|items| items.into_iter().map(Into::into).collect())
    }

    /// API wallet message center's business reminders across active API withdrawal wallets.
    pub async fn page_api_business_notifications(
        &self,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<ApiBusinessNotificationSummary>> {
        ApiBusinessNotificationService::new(self.ctx)
            .list_for_withdrawal_wallets(false, page, page_size)
            .await
            .map(summarize_page)
    }

    /// Unresolved business notifications across active API withdrawal wallets with balance refresh.
    pub async fn page_unresolved_api_business_notifications(
        &self,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<ApiBusinessNotificationSummary>> {
        ApiBusinessNotificationService::new(self.ctx)
            .list_unresolved_for_withdrawal_wallets_with_refresh(page, page_size)
            .await
            .map(summarize_page)
    }

    pub async fn mark_api_business_notification_read(&self, id: &str) -> ReturnType<()> {
        ApiBusinessNotificationService::new(self.ctx).mark_read_for_withdrawal_wallets(id).await
    }

    /// Count all API wallet notification kinds, including resolved unread history.
    /// Scoped to active API withdrawal wallets; ordinary wallet notifications are excluded.
    pub async fn count_unread_api_wallet_notifications(&self) -> ReturnType<i64> {
        ApiBusinessNotificationService::new(self.ctx).unread_count_for_withdrawal_wallets().await
    }

    /// Mark all business reminders read without resolving notifications.
    pub async fn mark_all_api_business_notifications_read(&self) -> ReturnType<()> {
        ApiBusinessNotificationService::new(self.ctx).mark_all_read_for_withdrawal_wallets().await
    }
}
