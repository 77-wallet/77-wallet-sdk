pub struct UserProperty {
    #[allow(dead_code)]
    pub(crate) package_id: String,
    pub(crate) content: String,
    pub(crate) client_id: String,
}

impl UserProperty {
    pub fn new(package_id: &str, content: &str, client_id: &str) -> Self {
        Self {
            package_id: package_id.to_string(),
            content: content.to_string(),
            client_id: client_id.to_string(),
        }
    }

    pub fn to_vec(&self) -> Vec<(String, String)> {
        let up = vec![
            ("content".to_string(), self.content.clone()),
            ("clientId".to_string(), self.client_id.clone()),
        ];

        up
    }
}
