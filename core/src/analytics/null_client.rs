use super::session::SessionId;

pub struct NullClient {
    session_id: SessionId,
}

impl NullClient {
    pub fn new() -> Self {
        Self {
            session_id: SessionId::random(),
        }
    }

    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }
}

impl Default for NullClient {
    fn default() -> Self {
        Self::new()
    }
}
