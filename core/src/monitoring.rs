use std::str::FromStr;

use anyhow::Result;
use log::error;

use sentry::ClientOptions;
use sentry_types::Dsn;

pub struct Monitoring {

}

impl Monitoring {

    // To have logging enabled this must be call after the logger setup
    pub fn try_setup_sentry() -> Result<()> {
        let dsn = option_env!("SENTRY_DSN");

        match dsn {
            None => {
                error!("sentry dsn is not provided via env variables, cannot setup sentry monitoring");
                Ok(())
            },
            Some(raw_dsn) => {
                let dsn = Dsn::from_str(raw_dsn)?;
            
                let opts = ClientOptions {
                    release: sentry::release_name!(),
                    dsn: Some(dsn),
                    attach_stacktrace: true,
                    debug: cfg!(debug_assertions),
                    ..Default::default()
                };
                let guard = sentry::init(opts);
                std::mem::forget(guard);
                Ok(())
            },
        }
    }
}
