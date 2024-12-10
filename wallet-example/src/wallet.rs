#[cfg(test)]
mod test {
    use wallet_api::setup_test_environment;

    #[tokio::test]
    async fn wallet() {
        let example = setup_test_environment("./request.json").await.unwrap();
        let _ = example.init().await;
    }
}
