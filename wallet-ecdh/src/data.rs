use crate::error::EncryptionError;

/// 加密的数据结构
#[derive(Debug, Clone, PartialEq)]
pub struct EncryptedData {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub key: Vec<u8>,
}

impl EncryptedData {
    /// 将加密数据序列化为字节数组
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();
        result.extend_from_slice(&(self.nonce.len() as u32).to_le_bytes());
        result.extend_from_slice(&(self.ciphertext.len() as u32).to_le_bytes());
        result.extend_from_slice(&(self.key.len() as u32).to_le_bytes());
        result.extend_from_slice(&self.nonce);
        result.extend_from_slice(&self.ciphertext);
        result.extend_from_slice(&self.key);
        result
    }

    /// 从字节数组反序列化加密数据
    pub fn from_bytes(data: &[u8]) -> Result<Self, EncryptionError> {
        if data.len() < 12 {
            return Err(EncryptionError::InvalidEncryptedData);
        }

        let nonce_len = u32::from_le_bytes(
            data[0..4].try_into().map_err(|_| EncryptionError::InvalidEncryptedData)?,
        ) as usize;
        let ciphertext_len = u32::from_le_bytes(
            data[4..8].try_into().map_err(|_| EncryptionError::InvalidEncryptedData)?,
        ) as usize;
        let key_len = u32::from_le_bytes(
            data[8..12].try_into().map_err(|_| EncryptionError::InvalidEncryptedData)?,
        ) as usize;

        if data.len() < 12 + nonce_len + ciphertext_len + key_len {
            return Err(EncryptionError::InvalidEncryptedData);
        }

        let nonce = data[12..12 + nonce_len].to_vec();
        let ciphertext = data[12 + nonce_len..12 + nonce_len + ciphertext_len].to_vec();
        let key = data[12 + nonce_len + ciphertext_len..12 + nonce_len + ciphertext_len + key_len]
            .to_vec();

        Ok(EncryptedData { key, nonce, ciphertext })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data() {
        let d = EncryptedData {
            nonce: vec![1, 2, 3, 4],
            ciphertext: vec![1, 2, 3, 4],
            key: vec![1, 2, 3, 4, 3, 4],
        };
        let v = d.to_bytes();
        let dd = EncryptedData::from_bytes(&v).unwrap();
        assert_eq!(d, dd);
    }

    #[test]
    fn test_invalid_data_rejected() {
        let err = EncryptedData::from_bytes(&[1, 2, 3]).expect_err("too short payload must fail");
        assert!(matches!(err, EncryptionError::InvalidEncryptedData));
    }
}
