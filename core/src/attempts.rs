use log::info;

const MAX_ATTEMPTS: i32 = 5;

#[derive(Default)]
pub struct Attempts {
    used_attempts: i32,
}

impl Attempts {
    pub fn try_consume_attempt(&mut self) -> bool {
        if self.used_attempts < MAX_ATTEMPTS {
            self.used_attempts += 1;
            info!("consumed attempt {} of {}", self.used_attempts, MAX_ATTEMPTS);
            return true;
        }

        info!("cannot consume attempt {} of {}", self.used_attempts, MAX_ATTEMPTS);
        false
    }

    pub fn can_retry(&self) -> bool {
        self.used_attempts < MAX_ATTEMPTS
    }
}
