pub mod s3;
pub mod types;
pub mod utils;
pub mod flow;
pub mod installs;
pub mod analytics;
pub mod environment;
pub mod protocols;
pub mod app;
pub mod channel;


pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
