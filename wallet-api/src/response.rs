#[derive(serde::Serialize, serde::Deserialize, Debug, Default, PartialEq, Eq, Clone)]
pub struct Response<T> {
    pub code: u32,
    pub message: String,
    pub result: Option<T>,
}

impl<T> From<Result<T, crate::ServiceError>> for Response<T>
where
    T: serde::Serialize + Sized,
{
    fn from(res: Result<T, crate::ServiceError>) -> Self {
        match res {
            Ok(ok) => ok.into(),
            Err(err) => {
                let (code, message) = err.into();
                Response {
                    code,
                    message,
                    result: None,
                }
            }
        }
    }
}

/// any type into ok response
impl<T> From<T> for Response<T>
where
    T: serde::Serialize + Sized,
{
    fn from(msg: T) -> Self {
        Self {
            code: 200,
            message: String::new(),
            result: Some(msg),
        }
    }
}

impl From<crate::ServiceError> for (u32, String) {
    fn from(err: crate::ServiceError) -> Self {
        // record log to upload to aliyun
        tracing::error!(?err, "api_error");
        // Separate network error types.
        if err.is_network_error() {
            return (502, err.to_string());
        }

        let (code, message) = match err {
            crate::ServiceError::Business(msg) => (msg.get_status_code(), msg.to_string()),
            crate::ServiceError::Parameter(_) => (422, err.to_string()),
            crate::ServiceError::System(_) => (500, err.to_string()),
            crate::ServiceError::Keystore(_) => (510, err.to_string()),
            crate::ServiceError::Utils(_) => (520, err.to_string()),
            crate::ServiceError::TransportBackend(bakend_err) => map_backend_error(bakend_err),
            crate::ServiceError::Transport(_) => (531, err.to_string()),
            crate::ServiceError::ChainInteract(_) => (540, err.to_string()),
            crate::ServiceError::ChainInstance(_) => (550, err.to_string()),
            crate::ServiceError::Core(_) => (610, err.to_string()),
            crate::ServiceError::Types(_) => (620, err.to_string()),
            crate::ServiceError::Database(_) => (630, err.to_string()),
            crate::ServiceError::Tree(_) => (640, err.to_string()),
            crate::ServiceError::Oss(_) => (650, err.to_string()),
        };
        (code, message)
    }
}

// 后端的某些错误映射
fn map_backend_error(err: wallet_transport_backend::Error) -> (u32, String) {
    match err {
        wallet_transport_backend::Error::BackendServiceError(service_err) => match service_err {
            wallet_transport_backend::error::BackendServiceError::NotPlatformAddress => (
                crate::MultisigAccountError::NotPlatFormAddress.get_status_code(),
                service_err.to_string(),
            ),
        },
        _ => (530, err.to_string()),
    }
}

impl<T> std::ops::FromResidual<Result<std::convert::Infallible, crate::ServiceError>>
    for Response<T>
{
    fn from_residual(residual: Result<std::convert::Infallible, crate::ServiceError>) -> Self {
        match residual {
            Err(err) => {
                let (code, message) = err.into();
                Response {
                    code,
                    message,
                    result: None,
                }
            }
        }
    }
}

impl<T> From<Result<T, crate::Errors>> for Response<T>
where
    T: serde::Serialize + Sized,
{
    fn from(res: Result<T, crate::Errors>) -> Self {
        match res {
            Ok(ok) => ok.into(),
            Err(err) => {
                let (code, message) = (err).into();
                Response {
                    code,
                    message,
                    result: None,
                }
            }
        }
    }
}

impl From<crate::Errors> for (u32, String) {
    fn from(err: crate::Errors) -> Self {
        let (code, message) = match err {
            crate::Errors::Service(e) => e.into(),
            crate::Errors::Parameter(_) => (204, err.to_string()),
        };
        (code, message)
    }
}

impl<T> std::ops::FromResidual<Result<std::convert::Infallible, crate::Errors>> for Response<T> {
    fn from_residual(residual: Result<std::convert::Infallible, crate::error::Errors>) -> Self {
        match residual {
            Err(err) => {
                let (code, message) = err.into();
                Response {
                    code,
                    message,
                    result: None,
                }
            }
        }
    }
}
