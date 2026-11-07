use std::{
    fs,
    ops::{Deref, DerefMut},
};

use serde::Serialize;
use wallet_crypto::KeystoreBuilder;

use crate::{
    directory_structure::LayoutStrategy,
    naming::{
        FileMeta,
        v2::{DerivedMetadata, KeyMeta, KeystoreData, ModernFileMeta},
    },
};

use super::{AccountTrait, RootTrait, WalletBranchOps, WalletTreeOps};

#[derive(Clone, Debug, Default)]
pub struct ModernWalletTree {
    pub layout: crate::directory_structure::v2::ModernLayout,
    pub naming: crate::naming::v2::ModernNaming,
    pub io: crate::file_ops::v2::ModernIo,
    pub tree: ModernWalletBranches,
}

#[derive(Clone, Debug, Default)]
pub struct ModernWalletBranches(std::collections::HashMap<String, ModernWalletBranch>);

impl Deref for ModernWalletBranches {
    type Target = std::collections::HashMap<String, ModernWalletBranch>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ModernWalletBranches {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ModernWalletTree {}

impl WalletTreeOps for ModernWalletTree {
    fn layout(&self) -> Box<dyn crate::directory_structure::LayoutStrategy> {
        Box::new(self.layout.clone())
    }

    // fn naming(&self) -> &dyn NamingStrategy {
    //     &self.naming
    // }

    fn io(&self) -> Box<dyn crate::file_ops::IoStrategy> {
        Box::new(self.io.clone())
    }

    fn traverse(wallet_dir: &std::path::Path) -> Result<Self, crate::Error>
    where
        Self: Sized,
    {
        let wallet_tree: Box<dyn std::any::Any> =
            crate::directory_structure::v2::ModernLayout.scan(wallet_dir)?;
        wallet_tree
            .downcast::<ModernWalletTree>() // 直接调用，无需类型转换
            .map(|boxed| *boxed) // 解包 Box
            .map_err(|_| crate::Error::FailedToDowncast)
    }

    fn delete_subkey(
        &mut self,
        // naming: Box<dyn crate::naming::NamingStrategy>,
        _wallet_address: &str,
        address: &str,
        chain_code: &str,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<(), crate::Error> {
        let base_path = file_path.as_ref();
        let meta_path = base_path.join("derived_meta.json");
        let subs_dir = base_path;

        // 1. 处理元数据文件
        let mut metadata: DerivedMetadata = if meta_path.exists() {
            let mut content = String::new();
            wallet_utils::file_func::read(&mut content, &meta_path)?;
            wallet_utils::serde_func::serde_from_str(&content).unwrap_or_default()
        } else {
            return Err(crate::Error::MetadataNotFound);
        };

        // 查找需要删除的条目并记录关联的密钥文件
        let mut keys_to_delete = Vec::new();
        for (account_idx, key_metas) in &mut metadata.accounts {
            // 保留不匹配的条目
            key_metas.retain(|meta| {
                let should_keep = !(meta.address == address && meta.chain_code == chain_code);
                if !should_keep {
                    // 生成要删除的key标识
                    let key = KeyMeta {
                        chain_code: meta.chain_code.clone(),
                        address: meta.address.clone(),
                        derivation_path: meta.derivation_path.clone(),
                    };
                    keys_to_delete.push((*account_idx, key.encode()));
                }
                should_keep
            });
        }

        tracing::info!("delete_subkey =============== 2");
        // 删除空账户
        metadata.accounts.retain(|_, metas| !metas.is_empty());

        // 原子写入新元数据
        let temp_meta_path = meta_path.with_extension("tmp");
        wallet_utils::file_func::write_all(
            &temp_meta_path,
            &wallet_utils::serde_func::serde_to_vec(&metadata)?,
        )?;
        fs::rename(&temp_meta_path, &meta_path).unwrap();

        // 2. 处理密钥文件
        for (account_idx, encoded_key) in keys_to_delete {
            let key_filename = format!("key{}.keystore", account_idx);
            let file_path = subs_dir.join(&key_filename);
            if !file_path.exists() {
                continue;
            }

            // 加载并解密数据
            let keystore = KeystoreBuilder::new_decrypt(&file_path, password).load()?;

            // 转换为可操作结构
            let mut keystore_data: KeystoreData = keystore.try_into()?;

            // 删除目标条目
            keystore_data.remove(&encoded_key);

            if keystore_data.is_empty() {
                wallet_utils::file_func::remove_file(file_path)?;
            } else {
                // 重新加密保存
                let rng = rand::thread_rng();
                KeystoreBuilder::new_encrypt(
                    &subs_dir,
                    password,
                    &wallet_utils::serde_func::serde_to_vec(&keystore_data)?,
                    rng,
                    wallet_crypto::KdfAlgorithm::Argon2id,
                    &key_filename,
                )
                .save()?;
            }
        }

        Ok(())
    }

    fn get_wallet_branch(
        &self,
        wallet_address: &str,
    ) -> Result<Box<dyn WalletBranchOps>, crate::Error> {
        self.tree
            .get(wallet_address)
            .map(|branch| Box::new(branch.clone()) as Box<dyn WalletBranchOps>)
            .ok_or(crate::Error::LocalNoWallet)
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (&String, &dyn WalletBranchOps)> + '_> {
        Box::new(self.tree.iter().map(|(k, v)| (k, v as &dyn WalletBranchOps)))
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModernWalletBranch {
    // 根账户信息
    pub root: ModernRoot,
    pub subs: Vec<ModernFileMeta>,
}

impl WalletBranchOps for ModernWalletBranch {
    fn get_root(&self) -> Box<dyn super::RootTrait> {
        Box::new(self.root.clone())
    }

    fn get_account(
        &self,
        address: &str,
        chain_code: &wallet_types::chain::chain::ChainCode,
    ) -> Option<Box<dyn super::AccountTrait>> {
        self.subs
            .iter()
            .find(|sub| {
                sub.get_address() == address && sub.chain_code() == Some(chain_code.to_string())
            })
            .map(|sub| Box::new(sub.clone()) as Box<dyn AccountTrait>)
    }

    fn get_accounts(&self) -> Vec<Box<dyn super::AccountTrait>> {
        self.subs.iter().map(|sub| Box::new(sub.clone()) as Box<dyn AccountTrait>).collect()
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModernRoot {
    // 根账户信息
    pub address: String,
}

impl RootTrait for ModernRoot {
    fn get_address(&self) -> &str {
        &self.address
    }
}

impl AccountTrait for ModernFileMeta {
    fn get_address(&self) -> &str {
        let t = self.address.as_deref();
        t.unwrap_or_default()
    }

    fn get_filemeta(&self) -> Box<dyn crate::naming::FileMeta> {
        Box::new(self.clone())
    }

    fn get_chain_code(&self) -> String {
        self.chain_code.as_ref().unwrap().to_string()
    }

    fn get_derivation_path(&self) -> &str {
        self.derivation_path.as_deref().unwrap_or_default()
    }
}
