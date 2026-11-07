use std::ops::{Deref, DerefMut};

use serde::Serialize;
use wallet_types::chain::chain::ChainCode;

use crate::{
    directory_structure::LayoutStrategy,
    naming::{FileMeta, FileType},
};

use super::{AccountTrait, RootTrait, WalletBranchOps, WalletTreeOps};

/// 钱包
///       根              子
///    pk    seed       pk  pk
///                      
/// 表示钱包的目录结构，将钱包名称映射到其下的账户目录结构。
#[derive(Debug, Default, PartialEq, Clone, Serialize)]
pub struct LegacyWalletTree {
    pub layout: crate::directory_structure::v1::LegacyLayout,
    pub naming: crate::naming::v1::LegacyNaming,
    pub io: crate::file_ops::v1::LegacyIo,
    pub tree: LegacyWalletBranches,
}

impl IntoIterator for LegacyWalletTree {
    type Item = (String, LegacyWalletBranch);
    type IntoIter = std::collections::hash_map::IntoIter<String, LegacyWalletBranch>;

    fn into_iter(self) -> Self::IntoIter {
        self.tree.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a LegacyWalletTree {
    type Item = (&'a String, &'a LegacyWalletBranch);
    type IntoIter = std::collections::hash_map::Iter<'a, String, LegacyWalletBranch>;

    fn into_iter(self) -> Self::IntoIter {
        self.tree.iter()
    }
}

impl<'a> IntoIterator for &'a mut LegacyWalletTree {
    type Item = (&'a String, &'a mut LegacyWalletBranch);
    type IntoIter = std::collections::hash_map::IterMut<'a, String, LegacyWalletBranch>;

    fn into_iter(self) -> Self::IntoIter {
        self.tree.iter_mut()
    }
}

#[derive(Debug, Default, PartialEq, Clone, Serialize)]
pub struct LegacyWalletBranches(std::collections::HashMap<String, LegacyWalletBranch>);

// impl LegacyWalletBranches {
//     pub fn iter(&self) -> impl Iterator<Item = (&String, &dyn WalletBranchOps)> {
//         self.0.iter().map(|(k, v)| (k, v as &dyn WalletBranchOps))
//     }
// }

impl Deref for LegacyWalletBranches {
    type Target = std::collections::HashMap<String, LegacyWalletBranch>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LegacyWalletBranches {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl LegacyWalletTree {
    // pub fn delete_subkeys(
    //     &mut self,
    //     wallet_address: &str,
    //     subs_path: &std::path::PathBuf,
    //     address: &str,
    //     chain_code: &ChainCode,
    // ) -> Result<(), crate::Error> {
    //     let wallet = self.get_mut_wallet_branch(wallet_address)?;
    //     wallet.delete_subkey(wallet_address, subs_path, address, chain_code)?;
    //     Ok(())
    // }

    pub fn get_wallet_branch(
        &self,
        wallet_address: &str,
    ) -> Result<&LegacyWalletBranch, crate::Error> {
        self.tree.get(wallet_address).ok_or(crate::error::Error::LocalNoWallet)
    }

    pub fn get_mut_wallet_branch(
        &mut self,
        wallet_address: &str,
    ) -> Result<&mut LegacyWalletBranch, crate::Error> {
        self.tree.get_mut(wallet_address).ok_or(crate::error::Error::LocalNoWallet)
    }

    /// 遍历指定目录结构，并将结果映射到数据结构中。
    ///
    /// # Arguments
    ///
    /// * `base_path` - 基础目录路径。
    ///
    /// # Returns
    ///
    /// 返回表示目录结构的数据结构，将钱包名称映射到其下的账户目录结构。
    ///
    /// # Example
    ///
    /// ```no_run
    /// let base_path = PathBuf::from("/path/to/wallets");
    /// let structure = traverse_directory_structure(base_path);
    /// ```
    pub fn traverse_directory_structure(
        wallet_dir: &std::path::PathBuf,
    ) -> Result<LegacyWalletTree, crate::Error> {
        let mut wallet_tree = LegacyWalletTree::default();

        let root = wallet_dir;
        for entry in wallet_utils::file_func::read_dir(root)? {
            let mut wallet_branch = LegacyWalletBranch::default();
            let entry = entry.map_err(|e| crate::Error::Utils(e.into()))?;
            let path = entry.path();

            tracing::info!("[traverse_directory_structure] path: {}", path.display());
            if path.is_dir() {
                let wallet_address = path.file_name().unwrap().to_string_lossy().to_string();
                let root_dir = path.join("root");
                let subs_dir = path.join("subs");

                wallet_utils::file_func::create_dir_all(&root_dir)?;
                wallet_utils::file_func::create_dir_all(&subs_dir)?;

                tracing::info!("[traverse_directory_structure] root_dir: {}", root_dir.display());
                if let Some(root_dir) = wallet_utils::file_func::read_dir(root_dir)?
                    .filter_map(Result::ok)
                    .map(|e| e.file_name())
                    .find(|e| e.to_string_lossy().ends_with("-phrase"))
                {
                    let pk_filename = root_dir.to_string_lossy().to_string();
                    wallet_branch.add_root_from_filename(&pk_filename)?;
                    tracing::info!("[traverse_directory_structure] pk_filename: {}", pk_filename);
                    tracing::info!(
                        "[traverse_directory_structure] root_dir 2: {}",
                        root_dir.to_string_lossy()
                    );
                } else {
                    wallet_branch.root_info = RootKeystoreInfo::new(&wallet_address);
                };

                tracing::info!("[traverse_directory_structure] wallet_branch: {:?}", wallet_branch);
                for subs_entry in wallet_utils::file_func::read_dir(subs_dir)? {
                    tracing::info!("[traverse_directory_structure] subs_entry: {:?}", subs_entry);
                    let subs_entry = subs_entry.map_err(|e| crate::Error::Utils(e.into()))?;
                    let subs_path = subs_entry.path();

                    tracing::info!(
                        "[traverse_directory_structure] subs_path: {}",
                        subs_path.display()
                    );
                    if subs_path.is_file()
                        && subs_path.file_name().unwrap().to_string_lossy().ends_with("-pk")
                    {
                        if let Err(e) = wallet_branch.add_subkey_from_filename(
                            &subs_path.file_name().unwrap().to_string_lossy(),
                        ) {
                            tracing::error!("[traverse_directory_structure] subs error: {e}");
                            continue;
                        };
                    }
                }

                // 将钱包分支添加到钱包树中
                wallet_tree.tree.insert(wallet_address.to_string(), wallet_branch);
            }
        }

        Ok(wallet_tree)
    }
}

#[derive(Debug, PartialEq, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyWalletBranch {
    // 根账户信息
    pub root_info: RootKeystoreInfo,
    pub accounts: Vec<SubsKeystoreInfo>,
}

impl Default for LegacyWalletBranch {
    fn default() -> Self {
        Self {
            root_info: RootKeystoreInfo { address: Default::default() },
            accounts: Default::default(),
        }
    }
}

// pub struct
impl LegacyWalletBranch {
    // 根据文件名解析并添加密钥
    pub fn add_subkey_from_filename(&mut self, filename: &str) -> Result<(), crate::Error> {
        let wallet_info = Self::extract_sub_address_and_derive_path_from_filename(filename)?;
        self.accounts.push(wallet_info);

        Ok(())
    }

    // 根据文件名解析并添加密钥
    pub fn add_root_from_filename(&mut self, filename: &str) -> Result<(), crate::Error> {
        let wallet_info = Self::extract_wallet_address_and_suffix_from_filename(filename)?;

        self.root_info = wallet_info;
        Ok(())
    }

    fn extract_sub_address_and_derive_path_from_filename(
        filename: &str,
    ) -> Result<SubsKeystoreInfo, crate::Error> {
        let parts: Vec<&str> = filename.split('-').collect();
        if parts.len() >= 4 {
            let chain_code = parts[0];
            let address = parts[1].to_string();
            let encoded_derivation_path = parts[2].to_string();
            let suffix = parts[3];

            let file_type = if suffix.ends_with("pk") {
                FileType::PrivateKey
            } else {
                return Err(crate::Error::FilenameInvalid);
            };
            let derivation_path =
                wallet_utils::parse_func::derivation_path_percent_decode(&encoded_derivation_path)?
                    .to_string();

            Ok(SubsKeystoreInfo {
                derivation_path,
                address,
                chain_code: chain_code.try_into()?,
                file_type,
            })
        } else {
            Err(crate::Error::FilenameInvalid)
        }
    }

    fn extract_wallet_address_and_suffix_from_filename(
        filename: &str,
    ) -> Result<RootKeystoreInfo, crate::Error> {
        let parts: Vec<&str> = filename.split('-').collect();
        if parts.len() >= 2 {
            let address = parts[0].to_string();
            let suffix = parts[1];
            if suffix.ends_with("phrase") {
                FileType::Phrase
            } else if suffix.ends_with("seed") {
                FileType::Seed
            } else {
                return Err(crate::Error::FilenameInvalid);
            };

            Ok(RootKeystoreInfo { address })
        } else {
            Err(crate::Error::FilenameInvalid)
        }
    }

    pub fn delete_subkey(
        &mut self,
        wallet_address: &str,
        subs_path: &std::path::Path,
        address: &str,
        chain_code: &ChainCode,
    ) -> Result<(), crate::Error> {
        if self.root_info.address == wallet_address {
            let mut removed_accounts = Vec::new();
            self.accounts.retain(|account| {
                let should_remove = account.address == address && &account.chain_code == chain_code;

                if should_remove {
                    removed_accounts.push(account.clone());
                }

                !should_remove
            });

            for removed_account in removed_accounts {
                let old_pk_name = removed_account.gen_name_with_derivation_path()?;
                let old_path = subs_path.join(old_pk_name);
                if let Err(e) = wallet_utils::file_func::remove_file(&old_path) {
                    tracing::error!("[delete_subkeys] Remove {old_path:?} error: {e}");
                };
            }
        }

        Ok(())
    }

    pub fn recover_subkey(
        &mut self,
        sub_address: &str,
        subs_path: std::path::PathBuf,
    ) -> Result<(), crate::Error> {
        if let Some(keystore_info) =
            self.accounts.iter_mut().find(|keystore_info| keystore_info.address == sub_address)
        {
            let old_pk_name = keystore_info.gen_name_with_derivation_path()?;
            let old_path = subs_path.join(old_pk_name);
            keystore_info.file_type = FileType::DerivedData;
            let new_pk_name = keystore_info.gen_name_with_derivation_path()?;
            let new_path = subs_path.join(new_pk_name);
            wallet_utils::file_func::rename(old_path, new_path)?;
        }

        Ok(())
    }

    pub fn get_account(&self, address: &str, chain_code: &ChainCode) -> Option<SubsKeystoreInfo> {
        self.accounts
            .iter()
            .find(|a| a.address == address && a.chain_code == *chain_code)
            .map(|info| info.to_owned())
    }
}

// pub struct LegacyAdapter {
//     naming: LegacyNaming,
//     layout: LegacyLayout,
// }

impl WalletTreeOps for LegacyWalletTree {
    // type Naming<'a> = dyn NamingStrategy + 'a;
    fn layout(&self) -> Box<dyn LayoutStrategy> {
        Box::new(self.layout.clone())
    }

    // fn naming<'b>(&'b self) -> &Self::Naming<'_> {
    // let naming_ref: &dyn NamingStrategy = &self.naming;
    // naming_ref as &Self::Naming<'_>
    // }
    // fn naming(&'a self) -> &'a N
    // where
    //     N: 'a,
    // {
    //     let naming_ref: &dyn NamingStrategy = &self.naming;
    //     // &self.naming]
    //     naming_ref as &'a N
    // }

    fn io(&self) -> Box<dyn crate::file_ops::IoStrategy> {
        Box::new(self.io.clone())
    }
    fn traverse(wallet_dir: &std::path::Path) -> Result<Self, crate::Error>
    where
        Self: Sized,
    {
        LegacyWalletTree::traverse_directory_structure(&wallet_dir.to_path_buf())
    }

    fn get_wallet_branch(
        &self,
        wallet_address: &str,
    ) -> Result<Box<dyn WalletBranchOps>, crate::Error> {
        self.tree
            .get(wallet_address)
            .ok_or(crate::Error::LocalNoWallet)
            .map(|info| Box::new(info.to_owned()) as Box<dyn WalletBranchOps>)
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (&String, &dyn WalletBranchOps)> + '_> {
        Box::new(self.tree.iter().map(|(k, v)| (k, v as &dyn WalletBranchOps)))
    }

    fn delete_subkey(
        &mut self,
        // naming: Box<dyn crate::naming::NamingStrategy>,
        wallet_address: &str,
        address: &str,
        chain_code: &str,
        file_path: &dyn AsRef<std::path::Path>,
        _password: &str,
    ) -> Result<(), crate::Error> {
        let wallet = self.tree.get_mut(wallet_address).ok_or(crate::Error::LocalNoWallet)?;

        wallet.delete_subkey(
            wallet_address,
            file_path.as_ref(),
            address,
            &chain_code.try_into()?,
        )?;

        Ok(())
    }
}

impl WalletBranchOps for LegacyWalletBranch {
    fn get_account(&self, address: &str, chain_code: &ChainCode) -> Option<Box<dyn AccountTrait>> {
        self.accounts
            .iter()
            .find(|a| a.address == address && a.chain_code == *chain_code)
            .map(|info| Box::new(info.clone()) as Box<dyn AccountTrait>)
    }

    fn get_root(&self) -> Box<dyn super::RootTrait> {
        Box::new(self.root_info.clone())
    }

    fn get_accounts(&self) -> Vec<Box<dyn AccountTrait>> {
        self.accounts.iter().map(|info| Box::new(info.clone()) as Box<dyn AccountTrait>).collect()
    }
}

#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct RootKeystoreInfo {
    pub address: String,
}

impl RootTrait for RootKeystoreInfo {
    fn get_address(&self) -> &str {
        &self.address
    }
}

impl RootKeystoreInfo {
    pub fn new(address: &str) -> Self {
        Self { address: address.to_string() }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct SubsKeystoreInfo {
    pub derivation_path: String,
    pub address: String,
    pub chain_code: ChainCode,
    pub file_type: FileType,
}

impl SubsKeystoreInfo {
    pub fn new(
        derivation_path: &str,
        chain_code: &ChainCode,
        address: &str,
        file_type: FileType,
    ) -> Self {
        Self {
            derivation_path: derivation_path.to_string(),
            address: address.to_string(),
            chain_code: *chain_code,
            file_type,
        }
    }

    pub fn gen_name_with_derivation_path(&self) -> Result<String, crate::Error> {
        let derivation_path =
            wallet_utils::parse_func::derivation_path_percent_encode(&self.derivation_path);

        let name = format!("{}-{}-{}-pk", self.chain_code, self.address, derivation_path);
        Ok(name)
    }
}

impl AccountTrait for SubsKeystoreInfo {
    fn get_address(&self) -> &str {
        &self.address
    }

    fn get_filemeta(&self) -> Box<dyn crate::naming::FileMeta> {
        Box::new(self.clone())
    }

    fn get_chain_code(&self) -> String {
        self.chain_code.to_string()
    }

    fn get_derivation_path(&self) -> &str {
        &self.derivation_path
    }
}

impl FileMeta for SubsKeystoreInfo {
    fn file_type(&self) -> &crate::naming::FileType {
        &crate::naming::FileType::DerivedData
    }

    fn address(&self) -> Option<String> {
        Some(self.address.clone())
    }

    fn chain_code(&self) -> Option<String> {
        let chain = self.chain_code.to_string().clone();
        Some(chain)
    }

    fn derivation_path(&self) -> Option<String> {
        Some(self.derivation_path.clone())
    }

    fn account_index(&self) -> Option<u32> {
        None
    }
}
