use anyhow::Result;
use wallet_api::{manager::WalletManager, testkit::mqtt::exec_wallet_order_payload};

use crate::{
    get_manager,
    harness::{AssertRole, GivenRole, SeedRole, ThenRole, WhenRole},
};

use super::{
    db::{
        assert_api_wallet_asset_symbol, assert_normal_wallet_asset_symbol, ensure_eth_chain_active,
        ensure_sol_chain_active, seed_api_wallet_sol_usdc_asset, seed_normal_eth_native_asset,
        seed_normal_eth_usdt_asset,
    },
    fixtures::AcctChangeFixture,
    task::wait_task_done,
};

pub(crate) struct AcctChangeScenario {
    _manager: WalletManager,
}

impl AcctChangeScenario {
    pub(crate) async fn new() -> Self {
        Self { _manager: get_manager().await }
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait AcctChangeGiven {
    async fn api_wallet_sol_usdc_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture>;

    async fn normal_eth_usdt_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture>;

    async fn normal_eth_native_asset_with_missing_token(&self) -> Result<AcctChangeFixture>;
}

#[async_trait::async_trait(?Send)]
impl AcctChangeGiven for GivenRole<'_, AcctChangeScenario> {
    async fn api_wallet_sol_usdc_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture> {
        self.scenario().seed().api_wallet_sol_usdc_asset_with_symbol_mismatch().await
    }

    async fn normal_eth_usdt_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture> {
        self.scenario().seed().normal_eth_usdt_asset_with_symbol_mismatch().await
    }

    async fn normal_eth_native_asset_with_missing_token(&self) -> Result<AcctChangeFixture> {
        self.scenario().seed().normal_eth_native_asset_with_missing_token().await
    }
}

#[async_trait::async_trait(?Send)]
trait AcctChangeSeed {
    async fn api_wallet_sol_usdc_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture>;

    async fn normal_eth_usdt_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture>;

    async fn normal_eth_native_asset_with_missing_token(&self) -> Result<AcctChangeFixture>;
}

#[async_trait::async_trait(?Send)]
impl AcctChangeSeed for SeedRole<'_, AcctChangeScenario> {
    async fn api_wallet_sol_usdc_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture> {
        let ctx = self.scenario()._manager.ctx();
        ensure_sol_chain_active(ctx).await?;
        let fixture = AcctChangeFixture::api_wallet_sol_usdc_symbol_mismatch();
        seed_api_wallet_sol_usdc_asset(ctx, &fixture).await?;
        Ok(fixture)
    }

    async fn normal_eth_usdt_asset_with_symbol_mismatch(&self) -> Result<AcctChangeFixture> {
        let ctx = self.scenario()._manager.ctx();
        ensure_eth_chain_active(ctx).await?;
        let fixture = AcctChangeFixture::normal_eth_usdt_symbol_mismatch();
        seed_normal_eth_usdt_asset(ctx, &fixture).await?;
        Ok(fixture)
    }

    async fn normal_eth_native_asset_with_missing_token(&self) -> Result<AcctChangeFixture> {
        let ctx = self.scenario()._manager.ctx();
        ensure_eth_chain_active(ctx).await?;
        let fixture = AcctChangeFixture::normal_eth_native_missing_token();
        seed_normal_eth_native_asset(ctx, &fixture).await?;
        Ok(fixture)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait AcctChangeWhen {
    async fn acct_change_payload_executes(&self, fixture: &AcctChangeFixture) -> Result<u8>;
}

#[async_trait::async_trait(?Send)]
impl AcctChangeWhen for WhenRole<'_, AcctChangeScenario> {
    async fn acct_change_payload_executes(&self, fixture: &AcctChangeFixture) -> Result<u8> {
        let ctx = self.scenario()._manager.ctx();
        exec_wallet_order_payload(ctx, &fixture.payload).await?;
        wait_task_done(ctx, &fixture.msg_id).await
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait AcctChangeThen {
    fn api_wallet_acct_change_succeeds(&self, status: u8);

    fn normal_wallet_acct_change_succeeds(&self, status: u8);

    fn normal_wallet_native_acct_change_succeeds(&self, status: u8);

    async fn api_wallet_asset_keeps_usdc_symbol(&self, fixture: &AcctChangeFixture) -> Result<()>;

    async fn normal_wallet_token_asset_keeps_usdt_symbol(
        &self,
        fixture: &AcctChangeFixture,
    ) -> Result<()>;

    async fn normal_wallet_native_asset_keeps_eth_symbol(
        &self,
        fixture: &AcctChangeFixture,
    ) -> Result<()>;
}

#[async_trait::async_trait(?Send)]
impl AcctChangeThen for ThenRole<'_, AcctChangeScenario> {
    fn api_wallet_acct_change_succeeds(&self, status: u8) {
        self.scenario().assert().api_wallet_acct_change_succeeds(status);
    }

    fn normal_wallet_acct_change_succeeds(&self, status: u8) {
        self.scenario().assert().normal_wallet_acct_change_succeeds(status);
    }

    fn normal_wallet_native_acct_change_succeeds(&self, status: u8) {
        self.scenario().assert().normal_wallet_native_acct_change_succeeds(status);
    }

    async fn api_wallet_asset_keeps_usdc_symbol(&self, fixture: &AcctChangeFixture) -> Result<()> {
        let ctx = self.scenario()._manager.ctx();
        self.scenario().assert().api_wallet_asset_keeps_usdc_symbol(ctx, fixture).await
    }

    async fn normal_wallet_token_asset_keeps_usdt_symbol(
        &self,
        fixture: &AcctChangeFixture,
    ) -> Result<()> {
        let ctx = self.scenario()._manager.ctx();
        self.scenario().assert().normal_wallet_token_asset_keeps_usdt_symbol(ctx, fixture).await
    }

    async fn normal_wallet_native_asset_keeps_eth_symbol(
        &self,
        fixture: &AcctChangeFixture,
    ) -> Result<()> {
        let ctx = self.scenario()._manager.ctx();
        self.scenario().assert().normal_wallet_native_asset_keeps_eth_symbol(ctx, fixture).await
    }
}

#[async_trait::async_trait(?Send)]
trait AcctChangeAssert {
    fn api_wallet_acct_change_succeeds(&self, status: u8);

    fn normal_wallet_acct_change_succeeds(&self, status: u8);

    fn normal_wallet_native_acct_change_succeeds(&self, status: u8);

    async fn api_wallet_asset_keeps_usdc_symbol(
        &self,
        ctx: &'static wallet_api::Context,
        fixture: &AcctChangeFixture,
    ) -> Result<()>;

    async fn normal_wallet_token_asset_keeps_usdt_symbol(
        &self,
        ctx: &'static wallet_api::Context,
        fixture: &AcctChangeFixture,
    ) -> Result<()>;

    async fn normal_wallet_native_asset_keeps_eth_symbol(
        &self,
        ctx: &'static wallet_api::Context,
        fixture: &AcctChangeFixture,
    ) -> Result<()>;
}

#[async_trait::async_trait(?Send)]
impl AcctChangeAssert for AssertRole<'_, AcctChangeScenario> {
    fn api_wallet_acct_change_succeeds(&self, status: u8) {
        assert_eq!(status, 2, "ApiWalletAcctChange task should succeed");
    }

    fn normal_wallet_acct_change_succeeds(&self, status: u8) {
        assert_eq!(status, 2, "AcctChange task should succeed");
    }

    fn normal_wallet_native_acct_change_succeeds(&self, status: u8) {
        assert_eq!(status, 2, "AcctChange native task should succeed");
    }

    async fn api_wallet_asset_keeps_usdc_symbol(
        &self,
        ctx: &'static wallet_api::Context,
        fixture: &AcctChangeFixture,
    ) -> Result<()> {
        assert_api_wallet_asset_symbol(ctx, fixture, "USDC").await
    }

    async fn normal_wallet_token_asset_keeps_usdt_symbol(
        &self,
        ctx: &'static wallet_api::Context,
        fixture: &AcctChangeFixture,
    ) -> Result<()> {
        assert_normal_wallet_asset_symbol(ctx, fixture, "USDT").await
    }

    async fn normal_wallet_native_asset_keeps_eth_symbol(
        &self,
        ctx: &'static wallet_api::Context,
        fixture: &AcctChangeFixture,
    ) -> Result<()> {
        assert_normal_wallet_asset_symbol(ctx, fixture, "ETH").await
    }
}
