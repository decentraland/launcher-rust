const DEFAULT_PROVIDER: &str = "dcl";

pub struct AppEnvironment {

}

impl AppEnvironment {

    pub fn provider() -> String {
        std::env::var("VITE_PROVIDER").unwrap_or_else(|_|  DEFAULT_PROVIDER.to_owned())
    }
}
