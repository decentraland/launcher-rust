pub struct SessionId {
    id: String,
}

impl SessionId {
    pub fn random() -> Self {
        SessionId {
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn value(&self) -> &str {
        self.id.as_str()
    }
}
