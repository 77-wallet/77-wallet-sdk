#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("FromUtf8 error: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),
    #[error("FromHex error: {0}")]
    FromHex(#[from] hex::FromHexError),
    #[error("FromHex error: {0}")]
    FromConstHex(#[from] alloy::hex::FromHexError),
    #[error("Method parse error: {0}")]
    HttpMethod(#[from] http::method::InvalidMethod),
    #[error("ToInt parse error: {0}")]
    ToInt(#[from] std::num::ParseIntError),
    #[error("Parse http body to bytes failed")]
    HttpBodyToBytesFailed,
    #[error("Parse Decimal to int 64 failed")]
    DecimalToI64Failed,
    #[error("Parse vector to array failed")]
    VecToArrayFailed,
    #[error("Address error: {0}")]
    AddressError(#[from] alloy::primitives::AddressError),
    #[error("address convert failed")]
    AddressConvertFailed,
    #[error("Decimal error: {0}")]
    Decimal(#[from] rust_decimal::Error),
    #[error("Custom enum error: {0}")]
    CustomEnum(String),
    #[error("unit convert failed{0}")]
    UnitConvertFailed(String),
}

impl ParseError {
    pub fn get_status_code(&self) -> u32 {
        match self {
            ParseError::FromUtf8(_) => 6300,
            ParseError::FromHex(_) => 6301,
            ParseError::FromConstHex(_) => 6301,
            ParseError::HttpMethod(_) => 6301,
            ParseError::ToInt(_) => 6304,
            ParseError::HttpBodyToBytesFailed => 6309,
            ParseError::DecimalToI64Failed => 6310,
            ParseError::VecToArrayFailed => 6311,
            ParseError::AddressError(_) => 6311,
            ParseError::AddressConvertFailed => 6311,
            ParseError::Decimal(_) => 6311,
            ParseError::CustomEnum(_) => 6311,
            ParseError::UnitConvertFailed(_) => 6311,
        }
    }
}
