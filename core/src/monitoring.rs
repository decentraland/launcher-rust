use std::borrow::Cow;
use std::str::FromStr;

use anyhow::Result;
use log::{error, info};

use sentry::{ClientOptions, protocol::User};
use sentry_types::Dsn;

use crate::{config, environment::AppEnvironment};

pub struct Monitoring {}

impl Monitoring {
    // To have logging enabled this must be call after the logger setup
    pub fn try_setup_sentry() -> Result<()> {
        let dsn = option_env!("SENTRY_DSN");

        match dsn {
            None => {
                error!(
                    "sentry dsn is not provided via env variables, cannot setup sentry monitoring"
                );
                Ok(())
            }
            Some(raw_dsn) => {
                info!("sentry dns is proiveded via env variables successfully");
                let dsn = Dsn::from_str(raw_dsn)?;

                let opts = ClientOptions {
                    release: sentry::release_name!(),
                    dsn: Some(dsn),
                    attach_stacktrace: true,
                    auto_session_tracking: true,
                    debug: cfg!(debug_assertions),
                    environment: Some(Cow::Owned(format!(
                        "{:?}",
                        AppEnvironment::launcher_environment()
                    ))),
                    ..Default::default()
                };
                let guard = sentry::init(opts);
                // keeps guard for the whole lifetime of the app
                std::mem::forget(guard);
                info!("sentry dns initialized successfully");

                // Set user scope
                let user_id = config::user_id_or_none();
                info!("sentry user_id {}", user_id);

                sentry::configure_scope(|scope| {
                    scope.set_user(Some(User {
                        id: Some(user_id),
                        ..Default::default()
                    }));
                });

                Ok(())
            }
        }
    }
}
