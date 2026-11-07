pub mod v1;
pub mod v2;

use serde::{Deserialize, Serialize};
use wallet_types::chain::chain::ChainCode;

use crate::{directory_structure::LayoutStrategy, file_ops::IoStrategy, naming::FileMeta};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WalletTreeStrategy {
    V1,
    V2,
}

impl WalletTreeStrategy {
    pub fn get_wallet_tree(
        self,
        wallet_dir: &std::path::Path,
    ) -> Result<Box<dyn WalletTreeOps>, crate::Error> {
        Ok(match self {
            WalletTreeStrategy::V1 => Box::new(v1::LegacyWalletTree::traverse(wallet_dir)?),
            WalletTreeStrategy::V2 => Box::new(v2::ModernWalletTree::traverse(wallet_dir)?),
        })
    }
}

pub trait WalletTreeOps: std::any::Any + std::fmt::Debug + std::marker::Send {
    fn layout(&self) -> Box<dyn LayoutStrategy>;

    fn io(&self) -> Box<dyn IoStrategy>;

    fn traverse(wallet_dir: &std::path::Path) -> Result<Self, crate::Error>
    where
        Self: Sized;

    fn delete_subkey(
        &mut self,
        wallet_address: &str,
        address: &str,
        chain_code: &str,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<(), crate::Error>;

    fn get_wallet_branch(
        &self,
        wallet_address: &str,
    ) -> Result<Box<dyn WalletBranchOps>, crate::Error>;

    fn iter(&self) -> Box<dyn Iterator<Item = (&String, &dyn WalletBranchOps)> + '_>;
}

pub trait WalletBranchOps {
    fn get_root(&self) -> Box<dyn RootTrait>;
    fn get_account(&self, address: &str, chain_code: &ChainCode) -> Option<Box<dyn AccountTrait>>;

    fn get_accounts(&self) -> Vec<Box<dyn AccountTrait>>;
}

pub trait AccountTrait: FileMeta + std::fmt::Debug + Send + Sync {
    // 定义公共接口方法
    fn get_address(&self) -> &str;
    fn get_filemeta(&self) -> Box<dyn crate::naming::FileMeta>;
    fn get_chain_code(&self) -> String;
    fn get_derivation_path(&self) -> &str;
}

pub trait RootTrait: std::fmt::Debug + Send + Sync {
    fn get_address(&self) -> &str;
}
