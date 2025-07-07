pub struct SessionId {
    id: String,
}

impl SessionId {
    pub fn random() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub const fn value(&self) -> &str {
        self.id.as_str()
    }
}
