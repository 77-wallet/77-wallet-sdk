pub mod http;
pub mod parse;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Parse error: {0}")]
    Parse(#[from] parse::ParseError),
    #[error("Http error: {0}")]
    Http(#[from] http::HttpError),
}

#[allow(unused)]
impl Error {
    pub fn get_status_code(&self) -> u32 {
        match self {
            Error::Parse(msg) => msg.get_status_code(),
            Error::Http(msg) => msg.get_status_code(),
        }
    }
}
