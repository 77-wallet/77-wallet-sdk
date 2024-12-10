#[derive(Debug, thiserror::Error)]
pub enum AnnouncementError {
    #[error("Announcement already exists")]
    AlreadyExist,
    #[error("Announcement not found")]
    NotFound,
}

impl AnnouncementError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            AnnouncementError::AlreadyExist => 3800,
            AnnouncementError::NotFound => 3801,
        }
    }
}
