#![allow(unused)]
use serde::Deserialize;
use serde_yaml;
use std::{fs, path::Path};

#[derive(Deserialize, Debug)]
struct Config {
    oss: OssConfig,
}

#[derive(Deserialize, Debug)]
struct OssConfig {
    access_key_id: String,
    access_key_secret: String,
    bucket_name: String,
    endpoint: String,
}

impl Config {
    pub fn new<S: AsRef<Path>>(config_path: S) -> Result<Self, crate::ServiceError> {
        let mut config_content = String::new();
        wallet_utils::file_func::read(&mut config_content, config_path)?;

        let config: Config = serde_yaml::from_str(&config_content)
            .map_err(|e| e.to_string())
            .unwrap();
        Ok(config)
    }
}
