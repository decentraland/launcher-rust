use crate::analytics::event::Event;
use crate::errors::DCLError;
use crate::fsm::values::Attempt;
use crate::logs::LogDestination;

use super::Executor;

impl Executor {
    pub(super) async fn fire_analytics_event(&self, event: Event) {
        self.analytics
            .lock()
            .await
            .track_and_flush_silent(event)
            .await;
    }
}

/// A capture for the final attempt, a breadcrumb before it; both land in the file log.
pub(super) fn report_to_sentry(error: &DCLError, attempt: Attempt, is_final: bool) {
    log::error!(
        target: LogDestination::File.as_target(),
        "Error during the flow. Attempt: {}, Cause {} {:#?}",
        attempt,
        error,
        error
    );
    let code = error.code();
    if is_final {
        sentry::with_scope(
            |scope| {
                scope.set_tag("error_code", code);
                scope.set_tag("attempt", attempt);
                scope.set_fingerprint(Some(&[code]));
            },
            || {
                sentry::capture_error(error);
            },
        );
    } else {
        sentry::add_breadcrumb(sentry::protocol::Breadcrumb {
            category: Some("flow.attempt".to_owned()),
            message: Some(format!("Error on attempt: {attempt}, cause: {error}")),
            level: sentry::Level::Warning,
            data: std::iter::once(("error_code".to_owned(), code.into())).collect(),
            ..Default::default()
        });
    }
}
