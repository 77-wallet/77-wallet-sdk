use crate::response_vo::standard_wallet::wallet::{GeneratePhraseRes, QueryPhraseRes};

use crate::{api::ReturnType, manager::WalletManager, service::wallet::WalletService};

impl WalletManager {
    /// Generates a mnemonic phrase based on the specified language and word count.
    ///
    /// This function calls the `generate_phrase` function from the wallet manager handler
    /// to generate a mnemonic phrase. The result is then converted into the appropriate response type.
    ///
    /// # Arguments
    ///
    /// * `language_code` - A `u8` representing the code of the language in which the phrase should be generated.
    /// * `count` - A `usize` specifying the number of words in the generated phrase.
    ///
    /// # Returns
    ///
    /// * `ReturnType<super::GeneratePhraseRes>` - A response containing the generated phrase.
    pub fn generate_phrase(
        &self,
        language_code: u8,
        count: usize,
    ) -> ReturnType<GeneratePhraseRes> {
        // Call the generate_phrase function from the wallet manager handler,
        // passing in the language code and word count.
        // The result is then converted into the response type `GeneratePhraseRes`.

        WalletService::new(self.ctx).generate_phrase(language_code, count)
    }

    /// Queries mnemonic phrases based on the specified language, keyword, and mode.
    ///
    /// This function calls the `query_phrases` function from the wallet manager handler
    /// to search for mnemonic phrases that match the given keyword in the specified language and mode.
    /// The result is then converted into the appropriate response type.
    ///
    /// # Arguments
    ///
    /// * `language_code` - A `u8` representing the code of the language in which to query phrases.
    /// * `keyword` - A `String` containing the keyword to search for within the mnemonic phrases.
    /// * `mode` - A `u8` specifying the mode of the search (e.g., exact match, prefix match).
    ///
    /// # Returns
    ///
    /// * `ReturnType<super::QueryPhraseRes>` - A response containing the queried phrases.
    pub fn query_phrases(
        &self,
        language_code: u8,
        keyword: &str,
        mode: u8,
    ) -> ReturnType<QueryPhraseRes> {
        // Call the query_phrases function from the wallet manager handler,
        // passing in the language code, keyword, and mode.
        // The result is then converted into the response type `QueryPhraseRes`.

        WalletService::new(self.ctx).query_phrases(language_code, keyword, mode)
    }

    /// Validates an array of mnemonic phrases and returns an array of valid phrases.
    ///
    /// This function takes an array of strings representing mnemonic phrases and checks
    /// which ones are valid according to the BIP39 wordlist. It returns an array containing
    /// only the valid phrases.
    ///
    /// # Arguments
    ///
    /// * `language_code` - A `u8` representing the code of the language for the mnemonic phrases.
    /// * `phrases` - A slice of strings containing the mnemonic phrases to validate.
    ///
    /// # Returns
    ///
    /// * `ReturnType<ValidatePhraseRes>` - A response containing the array of valid phrases.
    pub fn validate_phrases(
        &self,
        language_code: u8,
        phrases: Vec<&str>,
    ) -> ReturnType<Vec<String>> {
        WalletService::new(self.ctx).exact_query_phrase(language_code, phrases)
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use crate::testkit::env::get_manager;
    use anyhow::Result;

    #[tokio::test]
    async fn generate_phrase() -> Result<()> {
        let (wallet_manager, _test_params) = get_manager().await?;
        let phrase = wallet_manager.generate_phrase(1, 12)?;
        assert!(!phrase.phrases.is_empty());
        Ok(())
    }

    #[test]
    fn query_phrase() -> Result<()> {
        let language_code = 1;
        let keyword = "ap";
        let mode = wallet_core::language::QueryMode::StartsWith;
        // 调用被测函数
        let result =
            wallet_core::language::WordlistWrapper::new(language_code)?.query_phrase(keyword, mode);
        assert!(!result.is_empty());

        let mode = wallet_core::language::QueryMode::Contains;
        // 调用被测函数
        let result =
            wallet_core::language::WordlistWrapper::new(language_code)?.query_phrase(keyword, mode);
        assert!(!result.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn query_phrase_exact() -> Result<(), Box<dyn std::error::Error>> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let language_code = 1;
        // let phrases = vec![
        //     "abandon", "ability", "able", "about", "above", "absent", "absorb", "ad",
        // ];

        // wife smoke help special across among want screen solve anxiety worth enforce
        let phrases = vec![
            "wife", "smoke", "help", "special", "across", "among", "want", "screen", "solve",
            "anxiety", "worth", "enforce",
        ];
        // 调用被测函数
        let result = wallet_manager.validate_phrases(language_code, phrases)?;
        assert_eq!(result.len(), 12);
        Ok(())
    }
}
