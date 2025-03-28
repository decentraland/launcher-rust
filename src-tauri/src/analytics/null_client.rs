use super::session::SessionId;

pub struct NullClient {
    session_id: SessionId,
}

impl NullClient {
    
    pub fn new() -> Self {
        NullClient {
            session_id: SessionId::random(),
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
}
