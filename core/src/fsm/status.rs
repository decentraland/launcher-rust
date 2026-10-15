use crate::errors::DCLError;
use crate::types::{BuildType, LaunchStatus, Status, Step};

use super::state::{
    DeeplinkHandoff, ExplorerLaunch, NetworkResource, RunningInstances, State, UserIntention,
};
use super::values::Installation;

/// What the UI renders for this state; nothing before a launch intention exists.
pub fn status(state: &State) -> Option<Status> {
    if matches!(state.user_intention, UserIntention::Idle) {
        return None;
    }
    let launch_status = match final_error(state) {
        Some(error) => LaunchStatus::Error {
            message: error.user_message().to_owned(),
        },
        None => LaunchStatus::State { step: step(state) },
    };
    Some(Status::Launch(launch_status))
}

/// The first final error along the funnel; the retry button clears it.
fn final_error(state: &State) -> Option<&DCLError> {
    if let DeeplinkHandoff::Failed { error, .. } = &state.deeplink_handoff {
        return Some(error);
    }
    if let NetworkResource::Error { error, .. } = &state.latest_version {
        return Some(error);
    }
    if let Some(Installation::Failed { error, .. }) = target_installation(state) {
        return Some(error);
    }
    if let ExplorerLaunch::Failed { error } = &state.launch {
        return Some(error);
    }
    None
}

fn step(state: &State) -> Step {
    if matches!(state.running_instances, RunningInstances::Checking)
        || matches!(
            state.deeplink_handoff,
            DeeplinkHandoff::HandingOff { .. } | DeeplinkHandoff::HandedOff
        )
    {
        return Step::DeeplinkOpening;
    }
    if matches!(
        state.launch,
        ExplorerLaunch::Launching(_) | ExplorerLaunch::Launched(_)
    ) {
        return Step::Launching;
    }
    match target_installation(state) {
        Some(Installation::Downloading { percent, .. }) => Step::Downloading {
            progress: *percent,
            build_type: build_type(state),
        },
        Some(Installation::Installing { .. }) => Step::Installing {
            build_type: build_type(state),
        },
        _ => Step::Fetching,
    }
}

fn target_installation(state: &State) -> Option<&Installation> {
    let NetworkResource::Fetched(target) = &state.latest_version else {
        return None;
    };
    state.local_installations.scanned()?.get(target)
}

fn build_type(state: &State) -> BuildType {
    let any_ready = state
        .local_installations
        .scanned()
        .is_some_and(|installations| {
            installations
                .values()
                .any(|installation| matches!(installation, Installation::Ready(_)))
        });
    if any_ready {
        BuildType::Update
    } else {
        BuildType::New
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::fsm::state::LocalInstallations;
    use crate::fsm::values::{ReadyInstallation, UpdateStrategy};
    use crate::version::Version;

    fn version(value: &str) -> Version {
        Version::parse(value).unwrap_or(Version::Dev)
    }

    fn launching_state() -> State {
        State {
            user_intention: UserIntention::Launch(None),
            ..State::default()
        }
    }

    fn with_target(mut state: State, target: &str, installation: Installation) -> State {
        state.latest_version = NetworkResource::Fetched(version(target));
        let mut installations = BTreeMap::new();
        installations.insert(version(target), installation);
        state.local_installations = LocalInstallations::Scanned(installations);
        state
    }

    fn step_of(state: &State) -> Option<Step> {
        match status(state)? {
            Status::Launch(LaunchStatus::State { step }) => Some(step),
            Status::Launch(LaunchStatus::Error { .. }) | Status::Report(_) => None,
        }
    }

    fn zip(target: &str) -> UpdateStrategy {
        UpdateStrategy::ByZip {
            target: version(target),
        }
    }

    #[test]
    fn idle_renders_nothing() {
        assert!(status(&State::default()).is_none());
    }

    #[test]
    fn fresh_intention_is_fetching() {
        assert!(matches!(step_of(&launching_state()), Some(Step::Fetching)));
    }

    #[test]
    fn instances_check_and_handoff_open_the_deeplink() {
        let mut state = launching_state();
        state.running_instances = RunningInstances::Checking;
        assert!(matches!(step_of(&state), Some(Step::DeeplinkOpening)));

        state.running_instances = RunningInstances::Unknown;
        state.deeplink_handoff = DeeplinkHandoff::HandingOff { attempt: 2 };
        assert!(matches!(step_of(&state), Some(Step::DeeplinkOpening)));

        state.deeplink_handoff = DeeplinkHandoff::HandedOff;
        assert!(matches!(step_of(&state), Some(Step::DeeplinkOpening)));
    }

    #[test]
    fn fresh_download_is_new_and_carries_progress() {
        let state = with_target(
            launching_state(),
            "v2.0.0",
            Installation::Downloading {
                strategy: zip("v2.0.0"),
                percent: 42,
                attempt: 1,
            },
        );
        assert!(matches!(
            step_of(&state),
            Some(Step::Downloading {
                progress: 42,
                build_type: BuildType::New
            })
        ));
    }

    #[test]
    fn install_over_a_ready_version_is_an_update() {
        let mut state = with_target(
            launching_state(),
            "v2.0.0",
            Installation::Installing {
                strategy: zip("v2.0.0"),
                attempt: 1,
            },
        );
        if let Some(installations) = state.local_installations.scanned_mut() {
            installations.insert(
                version("v1.0.0"),
                Installation::Ready(ReadyInstallation(version("v1.0.0"))),
            );
        }
        assert!(matches!(
            step_of(&state),
            Some(Step::Installing {
                build_type: BuildType::Update
            })
        ));
    }

    #[test]
    fn launching_and_launched_render_launching() {
        let mut state = launching_state();
        state.launch = ExplorerLaunch::Launching(version("v1.0.0"));
        assert!(matches!(step_of(&state), Some(Step::Launching)));
        state.launch = ExplorerLaunch::Launched(version("v1.0.0"));
        assert!(matches!(step_of(&state), Some(Step::Launching)));
    }

    #[test]
    fn final_errors_render_their_user_message() {
        let error = Arc::new(DCLError::E3003_CANT_GET_VERSION);
        let expected = error.user_message().to_owned();

        let mut state = launching_state();
        state.latest_version = NetworkResource::Error {
            error: error.clone(),
            attempt: 3,
        };
        assert!(matches!(
            status(&state),
            Some(Status::Launch(LaunchStatus::Error { message })) if message == expected
        ));

        let state = with_target(
            launching_state(),
            "v2.0.0",
            Installation::Failed {
                error: error.clone(),
                attempt: 3,
            },
        );
        assert!(matches!(
            status(&state),
            Some(Status::Launch(LaunchStatus::Error { .. }))
        ));

        let mut state = launching_state();
        state.launch = ExplorerLaunch::Failed { error };
        assert!(matches!(
            status(&state),
            Some(Status::Launch(LaunchStatus::Error { .. }))
        ));
    }

    #[test]
    fn silent_attempts_keep_the_step() {
        let mut state = launching_state();
        state.latest_version = NetworkResource::Fetching { attempt: 2 };
        assert!(matches!(step_of(&state), Some(Step::Fetching)));
    }
}
