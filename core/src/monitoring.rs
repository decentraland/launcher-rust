use std::borrow::Cow;
use std::str::FromStr;

use anyhow::Result;
use log::{error, info};

use sentry::{ClientOptions, protocol::User};
use sentry_types::Dsn;

use crate::{
    config,
    environment::AppEnvironment,
    utils::{build_commit, build_pr},
};

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

                let env = format!("{:?}", AppEnvironment::launcher_environment()).to_lowercase();
                info!("sentry environment selected: {}", env);

                // Prod (PR_NUMBER=na) keeps `name@version` unchanged so existing
                // Sentry release-health history and alerts continue to work.
                // PR builds get a unique release per (pr, commit).
                let release = if build_pr() == "na" {
                    sentry::release_name!()
                } else {
                    Some(Cow::Owned(format!(
                        "{}@{}-pr.{}+{}",
                        env!("CARGO_PKG_NAME"),
                        env!("CARGO_PKG_VERSION"),
                        build_pr(),
                        build_commit()
                    )))
                };

                let opts = ClientOptions {
                    release,
                    dsn: Some(dsn),
                    attach_stacktrace: true,
                    auto_session_tracking: true,
                    debug: cfg!(debug_assertions),
                    environment: Some(Cow::Owned(env)),
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
                    if build_pr() != "na" {
                        scope.set_tag("git_commit", build_commit());
                        scope.set_tag("pr_number", build_pr());
                    }
                });

                Ok(())
            }
        }
    }
}
