use std::collections::BTreeMap;

use serde::Serialize;
use wallet_crypto::KeystoreBuilder;

use crate::naming::{
    FileType, NamingStrategy,
    v2::{DerivedMetadata, KeyMeta, KeyMetas, KeystoreData, ModernNaming},
};

use super::{BulkSubkey, IoStrategy};

#[derive(Debug, Default, PartialEq, Clone, Serialize)]
pub struct ModernIo;

#[cfg(test)]
mod tests;

mod root_protection;

impl IoStrategy for ModernIo {
    fn store(
        &self,
        name: &str,
        data: &dyn AsRef<[u8]>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let rng = rand::thread_rng();
        KeystoreBuilder::new_encrypt(file_path, password, data, rng, algorithm, name).save()?;
        Ok(())
    }

    fn load_account(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        subs_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<super::AccountData, crate::Error> {
        let pk_meta = ModernNaming::generate_filemeta(
            FileType::DerivedData,
            None,
            Some(account_index_map),
            None,
            None,
        )?;

        let pk_filename = ModernNaming::encode(pk_meta)?;
        let data =
            KeystoreBuilder::new_decrypt(subs_dir.as_ref().join(pk_filename), password).load()?;
        let derived_data: KeystoreData = data.try_into()?;

        let mut res = super::AccountData::default();
        for (k, v) in derived_data.iter() {
            match KeyMeta::decode(k) {
                Ok(meta) => {
                    res.insert(meta, v.to_vec());
                }
                Err(e) => tracing::error!("KeyMeta decode error: {e}"),
            }
        }

        Ok(res)
    }

    fn load_root(
        &self,
        wallet_address: &str,
        root_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<super::RootData, crate::Error> {
        let root_meta = ModernNaming::generate_filemeta(
            FileType::Root,
            Some(wallet_address.to_string()),
            None,
            None,
            None,
        )?;
        let root_filename = ModernNaming::encode(root_meta)?;
        let data =
            KeystoreBuilder::new_decrypt(root_dir.as_ref().join(root_filename), password).load()?;
        data.try_into()
    }

    fn load_subkey(
        &self,
        account_index_map: Option<&wallet_utils::address::AccountIndexMap>,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        subs_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<Vec<u8>, crate::Error> {
        let pk_meta = ModernNaming::generate_filemeta(
            FileType::DerivedData,
            Some(address.to_string()),
            account_index_map,
            Some(chain_code.to_string()),
            Some(derivation_path.to_string()),
        )?;
        let pk_filename = ModernNaming::encode(pk_meta)?;

        let data =
            KeystoreBuilder::new_decrypt(subs_dir.as_ref().join(pk_filename), password).load()?;

        let derived_data: KeystoreData = data.try_into()?;

        for (k, v) in derived_data.iter() {
            match KeyMeta::decode(k) {
                Ok(meta) => {
                    if meta.address == address
                        && meta.chain_code == chain_code
                        && meta.derivation_path == derivation_path
                    {
                        return Ok(v.to_vec());
                    }
                }
                Err(e) => tracing::error!("KeyMeta decode error: {e}"),
            }
        }

        Err(crate::Error::PrivateKeyNotFound)
    }

    fn store_root(
        &self,
        _address: &str,
        seed: &[u8],
        phrase: &str,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let data = super::RootData { phrase: phrase.to_string(), seed: seed.to_vec() };

        let file_name = "root.keystore";
        let data = wallet_utils::serde_func::serde_to_vec(&data)?;

        let rng = rand::thread_rng();
        KeystoreBuilder::new_encrypt(file_path, password, data, rng, algorithm, file_name)
            .save()?;

        Ok(())
    }

    fn store_subkey(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        data: &dyn AsRef<[u8]>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let account_idx = account_index_map.account_id;
        let base_path = file_path.as_ref();
        let meta_path = base_path.join("derived_meta.json");

        // 1. 处理元数据
        let mut metadata = if meta_path.exists() {
            let mut content = String::new();
            wallet_utils::file_func::read(&mut content, &meta_path)?;
            wallet_utils::serde_func::serde_from_str(&content).unwrap_or_default()
        } else {
            DerivedMetadata::default()
        };

        let meta = ModernNaming::generate_filemeta(
            FileType::DerivedData,
            Some(address.to_string()),
            Some(account_index_map),
            Some(chain_code.to_string()),
            Some(derivation_path.to_string()),
        )?;

        // 生成密钥文件名
        let key_filename = ModernNaming::encode(meta)?;
        // let key_filename = format!("key{}.keystore", account_idx);

        // 添加新条目
        metadata.accounts.entry(account_idx).or_insert(KeyMetas::default()).push(KeyMeta {
            chain_code: chain_code.to_string(),
            address: address.to_string(),
            derivation_path: derivation_path.to_string(),
        });

        // Prepare metadata without publishing it before keystore operations succeed.
        let contents = wallet_utils::serde_func::serde_to_string(&metadata)?;

        // 2. 处理密钥数据
        let data_path = base_path.join(&key_filename);
        let mut derived_data = crate::naming::v2::KeystoreData::default();
        if data_path.exists() {
            let keystore = KeystoreBuilder::new_decrypt(&data_path, password).load()?;
            derived_data = keystore.try_into()?;
        }

        let key = KeyMeta {
            chain_code: chain_code.to_string(),
            address: address.to_string(),
            derivation_path: derivation_path.to_string(),
        };

        derived_data.insert(key.encode(), data.as_ref().to_vec());

        let val = wallet_utils::serde_func::serde_to_vec(&derived_data)?;
        let rng = rand::thread_rng();
        KeystoreBuilder::new_encrypt(
            &base_path,
            password,
            val, // 原始二进制数据
            rng,
            algorithm,
            &key_filename, // 唯一标识
        )
        .save()?;
        // A decryption/encryption failure must not add an index entry for an unwritten key.
        wallet_utils::file_func::write_all(&meta_path, contents.as_bytes())?;
        Ok(())
    }

    fn store_subkeys_bulk(
        &self,
        subkeys: Vec<super::BulkSubkey>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let start = std::time::Instant::now();
        let base_path = file_path.as_ref();
        let meta_path = base_path.join("derived_meta.json");
        let subs_dir = base_path;
        wallet_utils::file_func::create_dir_all(subs_dir)?;

        // 1. 分组处理：按账户索引分组
        let mut grouped = BTreeMap::<u32, Vec<&BulkSubkey>>::new();
        for subkey in &subkeys {
            grouped.entry(subkey.account_index_map.account_id).or_default().push(subkey);
        }

        // 2. 准备元数据更新
        let mut metadata = if meta_path.exists() {
            let mut content = String::new();
            wallet_utils::file_func::read(&mut content, &meta_path)?;
            wallet_utils::serde_func::serde_from_str(&content).unwrap_or_default()
        } else {
            DerivedMetadata::default()
        };
        // 3. 批量处理密钥文件
        for (account_idx, subkeys) in grouped {
            let key_filename = format!("key{}.keystore", account_idx);
            let data_path = subs_dir.join(&key_filename);
            // 批量读取和更新数据
            let mut keystore_data = if data_path.exists() {
                let keystore = KeystoreBuilder::new_decrypt(&data_path, password).load()?;
                keystore.try_into()?
            } else {
                KeystoreData::default()
            };
            // 收集本批次的元数据更新
            for subkey in subkeys {
                let key = KeyMeta {
                    chain_code: subkey.chain_code.to_string(),
                    address: subkey.address.to_string(),
                    derivation_path: subkey.derivation_path.to_string(),
                };

                // 插入密钥数据
                keystore_data.insert(key.encode(), subkey.data.to_vec());

                metadata.accounts.entry(account_idx).or_insert(KeyMetas::default()).push(key);
            }

            // 4. 保存密钥文件
            let val = wallet_utils::serde_func::serde_to_vec(&keystore_data)?;
            let rng = rand::thread_rng();
            KeystoreBuilder::new_encrypt(
                &subs_dir,
                password,
                &val,
                rng,
                algorithm.clone(),
                &key_filename,
            )
            .save()?;
        }

        // 5. 原子写入元数据
        let temp_meta_path = meta_path.with_extension("tmp");
        wallet_utils::file_func::write_all(
            &temp_meta_path,
            &wallet_utils::serde_func::serde_to_vec(&metadata)?,
        )?;
        wallet_utils::file_func::rename(temp_meta_path, meta_path)?;
        tracing::debug!("store_subkeys_bulk cost: {:?}", start.elapsed());
        Ok(())
    }

    fn delete_root(
        &self,
        address: &str,
        root_dir: &dyn AsRef<std::path::Path>,
    ) -> Result<(), crate::Error> {
        wallet_utils::file_func::remove_file(root_dir.as_ref().join(ModernNaming::encode(
            ModernNaming::generate_filemeta(
                FileType::Root,
                Some(address.to_string()),
                None,
                None,
                None,
            )?,
        )?))?;

        Ok(())
    }

    fn delete_account(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        file_path: &dyn AsRef<std::path::Path>,
    ) -> Result<(), crate::Error> {
        let base_path = file_path.as_ref();
        let meta_path = base_path.join("derived_meta.json");
        let subs_dir = base_path;

        // 1. 原子操作元数据
        let mut metadata: DerivedMetadata = if meta_path.exists() {
            let mut content = String::new();
            wallet_utils::file_func::read(&mut content, &meta_path)?;
            wallet_utils::serde_func::serde_from_str(&content)?
        } else {
            return Err(crate::Error::MetadataNotFound);
        };

        // 移除指定账户的元数据
        metadata.accounts.remove(&account_index_map.account_id);

        // 2. 生成密钥文件名（保持与存储时相同的命名逻辑）
        let meta = ModernNaming::generate_filemeta(
            FileType::DerivedData,
            None, // address 字段在删除时不需要
            Some(account_index_map),
            None, // chain_code
            None, // derivation_path
        )?;

        let key_filename = ModernNaming::encode(meta)?;
        let data_path = subs_dir.join(key_filename);

        // 3. 并行处理文件操作
        let delete_result = std::thread::scope(|s| {
            // 元数据写入线程
            let meta_handle: std::thread::ScopedJoinHandle<'_, Result<(), wallet_utils::Error>> = s
                .spawn(|| {
                    let temp_path = meta_path.with_extension("tmp");
                    wallet_utils::file_func::write_all(
                        &temp_path,
                        &wallet_utils::serde_func::serde_to_vec(&metadata)?,
                    )?;
                    wallet_utils::file_func::rename(temp_path, meta_path)
                });

            // 密钥文件删除线程
            let data_handle: std::thread::ScopedJoinHandle<'_, Result<(), wallet_utils::Error>> = s
                .spawn(|| {
                    if data_path.exists() {
                        wallet_utils::file_func::remove_file(&data_path)?;
                    }
                    Ok(())
                });

            // 等待所有线程完成
            meta_handle.join().unwrap()?;
            data_handle.join().unwrap()?;
            Ok(())
        });

        // 4. 清理空目录
        if subs_dir.exists() && wallet_utils::file_func::read_dir(subs_dir)?.next().is_none() {
            wallet_utils::file_func::remove_dir_all(subs_dir)?;
        }

        delete_result
    }
}
