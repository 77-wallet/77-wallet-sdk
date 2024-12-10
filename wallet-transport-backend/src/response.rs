#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BackendResponseOk {
    pub code: Option<String>,
    pub data: Option<String>,
    pub success: bool,
    pub msg: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum BackendResponse {
    Success(BackendResponseOk),
}

impl BackendResponse {
    pub fn process<T: for<'de> serde::Deserialize<'de> + serde::Serialize>(
        self,
    ) -> Result<T, crate::error::Error> {
        match self {
            BackendResponse::Success(ok) => {
                if ok.success {
                    let res = match ok.data {
                        Some(data) => wallet_utils::cbc::AesCbcCryptor::new(
                            wallet_utils::cbc::AES_KEY,
                            wallet_utils::cbc::AES_IV,
                        )
                        .decrypt(&data)
                        .map_err(crate::Error::Utils)?,
                        None => wallet_utils::serde_func::serde_to_value(None::<T>)?,
                    };
                    Ok(wallet_utils::serde_func::serde_from_value(res)?)
                } else {
                    if let Some(code) = ok.code {
                        return Err(Self::match_error_code(&code, ok.msg));
                    }
                    Err(crate::Error::Backend(ok.msg))
                }
            }
        }
    }

    /// match backend errro code.
    fn match_error_code(code: &str, msg: Option<String>) -> crate::Error {
        match code {
            "5001" => crate::Error::BackendServiceError(
                crate::error::BackendServiceError::NotPlatformAddress,
            ),
            _ => crate::Error::Backend(msg),
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct EtherscanResponse<T> {
    pub status: String,
    pub message: String,
    pub result: T,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Data {
    pub items: serde_json::Value,
    pub module: Option<serde_json::Value>,
    pub success: bool,
    pub message: Option<String>,
}

impl Data {
    pub fn serde<T: for<'de> serde::Deserialize<'de>>(self) -> Result<T, crate::Error> {
        if self.success
            && let Some(module) = self.module
        {
            Ok(wallet_utils::serde_func::serde_from_value(module)?)
        } else {
            Err(crate::Error::Backend(self.message))
        }
    }
}
