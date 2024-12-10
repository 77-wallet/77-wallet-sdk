use crate::response_vo::wallet::{GeneratePhraseRes, QueryPhraseRes};

use crate::api::ReturnType;
use crate::service::wallet::WalletService;

impl crate::WalletManager {
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

        WalletService::new(self.repo_factory.resuource_repo())
            .generate_phrase(language_code, count)?
            .into()
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

        WalletService::new(self.repo_factory.resuource_repo())
            .query_phrases(language_code, keyword, mode)?
            .into()
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
        WalletService::new(self.repo_factory.resuource_repo())
            .exact_query_phrase(language_code, phrases)?
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::test::env::{setup_test_environment, TestData};

    #[tokio::test]
    async fn generate_phrase() {
        let TestData { wallet_manager, .. } = setup_test_environment(None, None, false, None)
            .await
            .unwrap();

        let phrase = wallet_manager.generate_phrase(1, 12);
        println!("{:?}", phrase);
    }

    #[test]
    fn query_phrase() -> Result<(), Box<dyn std::error::Error>> {
        let language_code = 1;
        let keyword = "ap";
        let mode = wallet_core::language::QueryMode::StartsWith;
        // 调用被测函数
        let result =
            wallet_core::language::WordlistWrapper::new(language_code)?.query_phrase(keyword, mode);
        println!("StartsWith result: {result:?}");

        let mode = wallet_core::language::QueryMode::Contains;
        // 调用被测函数
        let result =
            wallet_core::language::WordlistWrapper::new(language_code)?.query_phrase(keyword, mode);
        println!("Contains result: {result:?}");
        Ok(())
    }

    #[tokio::test]
    async fn query_phrase_exact() -> Result<(), Box<dyn std::error::Error>> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
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
        let result = wallet_manager.validate_phrases(language_code, phrases);
        println!("Exact result: {result:?}");
        Ok(())
    }
}
