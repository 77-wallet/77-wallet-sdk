#[derive(Clone)]
pub struct UserProperty {
    #[allow(dead_code)]
    pub(crate) content: String,
    pub(crate) client_id: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) app_version: String,
}

impl UserProperty {
    pub fn new(
        content: String,
        client_id: String,
        username: &str,
        password: String,
        app_version: &str,
    ) -> Self {
        Self {
            content,
            client_id,
            username: username.to_string(),
            password,
            app_version: app_version.to_string(),
        }
    }

    pub fn to_vec(&self) -> Vec<(String, String)> {
        vec![
            ("username".to_string(), self.username.clone()),
            ("password".to_string(), self.password.clone()),
            ("appVersion".to_string(), self.app_version.clone()),
        ]
    }
}
