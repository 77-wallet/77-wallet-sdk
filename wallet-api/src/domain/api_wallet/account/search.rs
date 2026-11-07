use wallet_database::repositories::api_wallet::account::ApiAccountRepo;

use crate::{
    domain::{account::AccountDomain, api_wallet::account::ApiAccountDomain},
    error::service::ServiceError,
    response_vo::api_wallet::account::ApiWalletAddressSearchResp,
};

impl ApiAccountDomain {
    /// 地址搜索：在指定 API 钱包 uid 范围内搜索账户地址
    pub async fn search_address(
        ctx: &crate::context::Context,
        uid: &str,
        keyword: &str,
    ) -> Result<ApiWalletAddressSearchResp, ServiceError> {
        tracing::info!(
            uid = %uid,
            keyword = %keyword,
            "ApiAccountDomain::search_address"
        );

        // 地址格式预校验：只在格式有效时才触发数据库查询
        if !Self::is_valid_address_format(keyword) {
            tracing::info!(keyword = %keyword, "Invalid address format, returning empty result");
            return Ok(ApiWalletAddressSearchResp { items: vec![] });
        }

        let pool = ctx.api_wallet_pool()?;

        let entities = ApiAccountRepo::search_address_by_uid(&pool, uid, keyword).await?;

        let items = entities.into_iter().map(|entity| entity.into()).collect();

        Ok(ApiWalletAddressSearchResp { items })
    }

    /// 地址格式预校验
    fn is_valid_address_format(keyword: &str) -> bool {
        // 最小长度检查
        if keyword.len() < 20 {
            return false;
        }

        // 检查是否包含明显非法字符（不允许空格、@、#、$ 等特殊字符）
        let has_invalid_chars = keyword.chars().any(|c| {
            matches!(
                c,
                ' ' | '\t'
                    | '\n'
                    | '\r'
                    | '@'
                    | '#'
                    | '$'
                    | '%'
                    | '^'
                    | '&'
                    | '*'
                    | '('
                    | ')'
                    | '+'
                    | '='
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '|'
                    | ';'
                    | ':'
                    | '"'
                    | '\''
                    | '<'
                    | '>'
                    | ','
                    | '?'
                    | '/'
            )
        });

        !has_invalid_chars
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::api_wallet::account::ApiAccountDomain;

    #[test]
    fn test_is_valid_address_format() {
        // 有效的以太坊地址格式（十六进制）
        assert!(ApiAccountDomain::is_valid_address_format(
            "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A24"
        ));
        assert!(ApiAccountDomain::is_valid_address_format(
            "0x17F6A199862fd0FFb2D5c79F3dbbe37597162a24"
        ));
        assert!(ApiAccountDomain::is_valid_address_format(
            "17f6a199862FD0ffb2d5C79f3DBBE37597162A24"
        ));

        // 有效的 TRON 地址格式（Base58）
        assert!(ApiAccountDomain::is_valid_address_format("TQn9Y2khEsLJW1ChVWFMSMeRDow5KcbLSE"));
        assert!(ApiAccountDomain::is_valid_address_format("tqn9y2khesljw1chvwfmsmerdow5kcblse"));

        // 有效的 Solana 地址格式（Base58）
        assert!(ApiAccountDomain::is_valid_address_format(
            "7Z3s1vZZZ68q9n99fW7QJ5QJ5QJ5QJ5QJ5QJ5QJ5QJ"
        ));

        // 无效的地址格式（包含明显非法字符）
        assert!(!ApiAccountDomain::is_valid_address_format("short"));
        assert!(!ApiAccountDomain::is_valid_address_format("1234567890"));
        assert!(!ApiAccountDomain::is_valid_address_format("0x"));
        assert!(!ApiAccountDomain::is_valid_address_format("0x123"));
        assert!(!ApiAccountDomain::is_valid_address_format("invalid@address"));
        assert!(!ApiAccountDomain::is_valid_address_format(
            "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A24 "
        ));
        assert!(!ApiAccountDomain::is_valid_address_format(
            "TQn9Y2khEsLJW1ChVWFMSMeRDow5KcbLSE test"
        ));
        assert!(!ApiAccountDomain::is_valid_address_format(
            "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A2#"
        ));
        assert!(!ApiAccountDomain::is_valid_address_format(
            "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A2&"
        ));
    }
}
