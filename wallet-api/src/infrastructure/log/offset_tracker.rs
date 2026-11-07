use std::path::PathBuf;
use tokio::io::AsyncReadExt as _;

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct OffsetTracker {
    offset: u64,
    path: PathBuf,
    uid: String,
}

impl OffsetTracker {
    pub async fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut tracker = OffsetTracker { offset: 0, path, uid: String::new() };
        tracker.load().await;
        tracker
    }

    pub fn get_offset(&self) -> u64 {
        self.offset
    }

    pub fn set_offset(&mut self, offset: u64) {
        self.offset = offset;
    }

    pub fn set_uid(&mut self, uid: String) {
        self.uid = uid;
    }

    pub fn get_uid(&self) -> &str {
        self.uid.as_str()
    }

    pub async fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self) {
            let _ = tokio::fs::write(&self.path, json).await;
        }
    }

    async fn load(&mut self) {
        if let Ok(mut f) = tokio::fs::File::open(&self.path).await {
            let mut contents = String::new();
            let _ = f.read_to_string(&mut contents).await;
            if let Ok(tracker) = serde_json::from_str::<Self>(&contents) {
                self.offset = tracker.offset;
                self.uid = tracker.uid;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OffsetTracker;

    #[tokio::test]
    async fn load_restores_offset_and_uid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("offset.json");

        let mut tracker = OffsetTracker::new(path.clone()).await;
        tracker.set_offset(42);
        tracker.set_uid("log-file-uid".to_string());
        tracker.save().await;

        let loaded = OffsetTracker::new(path).await;

        assert_eq!(loaded.get_offset(), 42);
        assert_eq!(loaded.get_uid(), "log-file-uid");
    }
}
