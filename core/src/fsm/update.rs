use std::sync::Arc;

use crate::analytics::event::Event;
use crate::deeplink_bridge::should_use_deeplink_bridge;
use crate::delta_manifest::DeltaManifest;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::utils;
use crate::version::Version;

use super::cmd::{Cmd, FetchRequest};
use super::msg::Msg;
use super::state::{LaunchStep, NetworkResource, State, UserIntention};
use super::values::{Attempt, Installation, Percent, ReadyInstallation, UpdateStrategy};

pub const SILENT_ATTEMPTS: Attempt = 3;

pub fn update(mut state: State, msg: Msg) -> (State, Cmd) {
    let cmd = match msg {
        Msg::LaunchIntentionReceived(deeplink) => {
            on_launch_intention_received(&mut state, deeplink)
        }
        Msg::DeeplinkReceived(deeplink) => on_deeplink_received(&mut state, deeplink),
        Msg::RetryIntentionReceived => on_retry_intention_received(&mut state),
        Msg::LatestVersionFetched(result) => on_latest_version_fetched(&mut state, result),
        Msg::DeltaManifestFetched(result) => on_delta_manifest_fetched(&mut state, result),
        Msg::LocalInstallationPresenceConfirmed(ready) => {
            on_local_installation_presence_confirmed(&mut state, ready)
        }
        Msg::VersionDownloadCompleted(strategy) => {
            on_version_download_completed(&mut state, strategy)
        }
        Msg::DownloadProgressed(strategy, percent) => {
            on_download_progressed(&mut state, &strategy, percent)
        }
        Msg::VersionDownloadFailed(strategy, error) => {
            on_version_download_failed(&mut state, strategy, error)
        }
        Msg::RunningInstancesChecked { any } => on_running_instances_checked(&mut state, any),
        Msg::DeeplinkHandedOff => {
            apply_step(&mut state, LaunchStep::DeepLinkPass);
            Cmd::None
        }
        Msg::DeeplinkHandoffFailed(error) => on_deeplink_handoff_failed(&mut state, error),
        Msg::VersionInstallationCompleted(version, ready) => {
            on_version_installation_completed(&mut state, version, ready)
        }
        Msg::VersionInstallationFailed(strategy, error) => {
            on_version_installation_failed(&mut state, strategy, error)
        }
        Msg::VersionLaunched(_) => {
            apply_step(&mut state, LaunchStep::Launched);
            Cmd::None
        }
        Msg::VersionLaunchFailed(_, error) => on_version_launch_failed(&mut state, error),
    };
    (state, cmd)
}

// Intention and steps

const fn launch_step(state: &State) -> Option<&LaunchStep> {
    match &state.user_intention {
        UserIntention::Launch { step, .. } => Some(step),
        UserIntention::Idle => None,
    }
}

fn apply_step(state: &mut State, next: LaunchStep) {
    if let UserIntention::Launch { step, .. } = &mut state.user_intention {
        *step = next;
    }
}

fn intended_deeplink(state: &State) -> Option<DeepLink> {
    match &state.user_intention {
        UserIntention::Launch { deeplink, .. } => deeplink.clone(),
        UserIntention::Idle => None,
    }
}

const fn is_launch_step_terminal(step: &LaunchStep) -> bool {
    matches!(
        step,
        LaunchStep::DeepLinkPass | LaunchStep::Launched | LaunchStep::Failed { .. }
    )
}

/// The step waits for the user: its own failure or a final error in the resource it depends on.
fn is_waiting_for_user_retry(state: &State) -> bool {
    match launch_step(state) {
        Some(LaunchStep::Failed { .. }) => true,
        Some(LaunchStep::Fetching { .. }) => {
            matches!(state.latest_version, NetworkResource::Error { .. })
        }
        Some(LaunchStep::Downloading | LaunchStep::Installing) => latest_fetched_version(state)
            .is_some_and(|target| {
                matches!(
                    state.installations.get(&target),
                    Some(Installation::Failed { .. })
                )
            }),
        _ => false,
    }
}

fn latest_fetched_version(state: &State) -> Option<Version> {
    match &state.latest_version {
        NetworkResource::Fetched(version) => Some(version.clone()),
        _ => None,
    }
}

fn on_launch_intention_received(state: &mut State, deeplink: Option<DeepLink>) -> Cmd {
    if launch_step(state).is_some_and(|step| !is_launch_step_terminal(step)) {
        return Cmd::Ignore("launch already in progress");
    }
    begin_launch_intention(state, deeplink)
}

fn on_deeplink_received(state: &mut State, deeplink: DeepLink) -> Cmd {
    if let UserIntention::Launch {
        deeplink: current,
        step,
    } = &mut state.user_intention
    {
        if !is_launch_step_terminal(step) {
            *current = Some(deeplink);
            return Cmd::Ignore(
                "launch in progress continues; LaunchVersion will carry the new deeplink",
            );
        }
    }
    begin_launch_intention(state, Some(deeplink))
}

fn on_retry_intention_received(state: &mut State) -> Cmd {
    if !is_waiting_for_user_retry(state) {
        return Cmd::Ignore("retry not awaited: nothing has failed");
    }
    state.latest_version = NetworkResource::Unknown;
    state.delta_manifest = NetworkResource::Unknown;
    state.installations.clear();
    let deeplink = intended_deeplink(state);
    Cmd::Multi(vec![
        fire_analytics_event(Event::RETRY_FLOW_BUTTON_CLICK {
            version: utils::app_version().to_owned(),
        }),
        begin_launch_intention(state, deeplink),
    ])
}

fn begin_launch_intention(state: &mut State, deeplink: Option<DeepLink>) -> Cmd {
    let has_deeplink = deeplink.is_some();
    state.user_intention = UserIntention::Launch {
        deeplink,
        step: LaunchStep::DeeplinkOpening { attempt: 1 },
    };
    if has_deeplink {
        return Cmd::CheckRunningInstances;
    }
    enter_fetching_step(state)
}

// Passthrough

fn on_running_instances_checked(state: &mut State, any: bool) -> Cmd {
    if !matches!(launch_step(state), Some(LaunchStep::DeeplinkOpening { .. })) {
        return Cmd::Ignore("stale instances check: launch is past DeeplinkOpening");
    }
    match intended_deeplink(state) {
        Some(deeplink) if should_use_deeplink_bridge(&deeplink, &state.config.args, any) => {
            Cmd::HandoffDeeplink(deeplink)
        }
        _ => enter_fetching_step(state),
    }
}

fn on_deeplink_handoff_failed(state: &mut State, error: DCLError) -> Cmd {
    let Some(LaunchStep::DeeplinkOpening { attempt }) = launch_step(state) else {
        return Cmd::Ignore("stale handoff failure: launch is past DeeplinkOpening");
    };
    let attempt = *attempt;
    let error = Arc::new(error);
    let report = report_attempt_error(&error, attempt);
    if is_final_attempt(attempt, &error) {
        apply_step(state, LaunchStep::Failed { error, attempt });
        return report;
    }
    let Some(deeplink) = intended_deeplink(state) else {
        return report;
    };
    apply_step(
        state,
        LaunchStep::DeeplinkOpening {
            attempt: attempt.saturating_add(1),
        },
    );
    Cmd::Multi(vec![report, Cmd::HandoffDeeplink(deeplink)])
}

// Fetch: local presence, latest version and the delta manifest

fn enter_fetching_step(state: &mut State) -> Cmd {
    apply_step(state, LaunchStep::Fetching { scanned: false });
    if !matches!(state.latest_version, NetworkResource::Unknown) {
        return Cmd::EnsureLocalInstallations;
    }
    Cmd::Multi(vec![
        Cmd::EnsureLocalInstallations,
        request_latest_version_fetch(state, 1),
    ])
}

fn on_local_installation_presence_confirmed(
    state: &mut State,
    ready: Vec<ReadyInstallation>,
) -> Cmd {
    state
        .installations
        .retain(|_, installation| !matches!(installation, Installation::Ready(_)));
    for installation in ready {
        state
            .installations
            .insert(installation.0.clone(), Installation::Ready(installation));
    }
    if matches!(launch_step(state), Some(LaunchStep::Fetching { .. })) {
        apply_step(state, LaunchStep::Fetching { scanned: true });
    }
    leave_fetching_step_if_ready(state)
}

/// Highest ready release; dev builds are never a delta base.
fn highest_ready_release(state: &State) -> Option<Version> {
    state
        .installations
        .iter()
        .rev()
        .find(|(version, installation)| {
            !version.is_dev() && matches!(installation, Installation::Ready(_))
        })
        .map(|(version, _)| version.clone())
}

fn on_latest_version_fetched(state: &mut State, result: Result<Version, DCLError>) -> Cmd {
    match result {
        Ok(version) => {
            let success = fire_analytics_event(Event::FETCH_VERSION_SUCCESS {
                version: version.to_string(),
            });
            state.latest_version = NetworkResource::Fetched(version);
            Cmd::Multi(vec![success, leave_fetching_step_if_ready(state)])
        }
        Err(error) => {
            let attempt = resource_fetch_attempt(&state.latest_version);
            let error = Arc::new(error);
            let failure = fire_analytics_event(Event::FETCH_VERSION_ERROR {
                error: error.to_string(),
            });
            let report = report_attempt_error(&error, attempt);
            if is_final_attempt(attempt, &error) {
                state.latest_version = NetworkResource::Error { error, attempt };
                return Cmd::Multi(vec![failure, report]);
            }
            let retry = request_latest_version_fetch(state, attempt.saturating_add(1));
            Cmd::Multi(vec![failure, report, retry])
        }
    }
}

fn request_latest_version_fetch(state: &mut State, attempt: Attempt) -> Cmd {
    state.latest_version = NetworkResource::Fetching { attempt };
    Cmd::Multi(vec![
        fire_analytics_event(Event::FETCH_VERSION_START),
        Cmd::Fetch(FetchRequest::LatestVersion),
    ])
}

const fn resource_fetch_attempt<T>(resource: &NetworkResource<T>) -> Attempt {
    match resource {
        NetworkResource::Fetching { attempt } => *attempt,
        _ => 1,
    }
}

fn on_delta_manifest_fetched(
    state: &mut State,
    result: Result<Option<DeltaManifest>, DCLError>,
) -> Cmd {
    match result {
        Ok(manifest) => {
            state.delta_manifest = NetworkResource::Fetched(manifest.map(Arc::new));
            leave_fetching_step_if_ready(state)
        }
        Err(error) => {
            let attempt = resource_fetch_attempt(&state.delta_manifest);
            let error = Arc::new(error);
            match highest_ready_release(state) {
                Some(from) if !is_final_attempt(attempt, &error) => Cmd::Multi(vec![
                    Cmd::ReportToSentry {
                        error,
                        attempt,
                        is_final: false,
                    },
                    request_delta_manifest_fetch(state, from, attempt.saturating_add(1)),
                ]),
                _ => {
                    state.delta_manifest = NetworkResource::Error {
                        error: error.clone(),
                        attempt,
                    };
                    Cmd::Multi(vec![
                        report_final_error_to_sentry(error),
                        leave_fetching_step_if_ready(state),
                    ])
                }
            }
        }
    }
}

fn request_delta_manifest_fetch(
    state: &mut State,
    current_version: Version,
    attempt: Attempt,
) -> Cmd {
    state.delta_manifest = NetworkResource::Fetching { attempt };
    Cmd::Fetch(FetchRequest::DeltaManifest { current_version })
}

/// Leaves the fetch step once the presence scan and the latest version are both in.
fn leave_fetching_step_if_ready(state: &mut State) -> Cmd {
    match launch_step(state) {
        Some(LaunchStep::Fetching { scanned: true }) => {}
        Some(LaunchStep::Fetching { scanned: false }) => {
            return Cmd::Ignore("fetching step waits for the local presence scan");
        }
        _ => return Cmd::Ignore("stale fetch result: launch is past Fetching"),
    }
    let Some(target) = latest_fetched_version(state) else {
        return Cmd::Ignore("fetching step waits for the latest version");
    };
    if matches!(
        state.installations.get(&target),
        Some(Installation::Ready(_))
    ) {
        let version = target.to_string();
        return Cmd::Multi(vec![
            fire_analytics_event(Event::DOWNLOAD_VERSION_SKIPPED {
                version: version.clone(),
            }),
            fire_analytics_event(Event::INSTALL_VERSION_SKIPPED { version }),
            enter_launching_step(state, target),
        ]);
    }
    match decide_update_strategy(state, &target) {
        UpdateDecision::Wait => Cmd::Ignore("fetching step waits for the delta manifest"),
        UpdateDecision::FetchManifest(from) => request_delta_manifest_fetch(state, from, 1),
        UpdateDecision::Download(strategy, leaf) => Cmd::Multi(vec![
            fire_analytics_event(Event::UPDATE_STRATEGY_DECIDED {
                version: target.to_string(),
                strategy: strategy.name().to_owned(),
                leaf: leaf.to_owned(),
            }),
            enter_downloading_step(state, strategy, 1),
        ]),
    }
}

enum UpdateDecision {
    Download(UpdateStrategy, &'static str),
    FetchManifest(Version),
    Wait,
}

fn decide_update_strategy(state: &State, target: &Version) -> UpdateDecision {
    let zip = || UpdateStrategy::ByZip {
        target: target.clone(),
    };
    if !state.config.use_delta_mode {
        return UpdateDecision::Download(zip(), "delta_disabled");
    }
    let Some(from) = highest_ready_release(state) else {
        return UpdateDecision::Download(zip(), "fresh_install");
    };
    match &state.delta_manifest {
        NetworkResource::Unknown => UpdateDecision::FetchManifest(from),
        NetworkResource::Fetching { .. } => UpdateDecision::Wait,
        NetworkResource::Fetched(None) => UpdateDecision::Download(zip(), "manifest_missing"),
        NetworkResource::Fetched(Some(manifest)) if manifest.lists_base(&from) => {
            UpdateDecision::Download(
                UpdateStrategy::ByDelta {
                    target: target.clone(),
                    from,
                    manifest: manifest.clone(),
                },
                "base_listed",
            )
        }
        NetworkResource::Fetched(Some(_)) => UpdateDecision::Download(zip(), "base_not_listed"),
        NetworkResource::Error { .. } => UpdateDecision::Download(zip(), "manifest_unavailable"),
    }
}

// Download and install

fn enter_downloading_step(state: &mut State, strategy: UpdateStrategy, attempt: Attempt) -> Cmd {
    apply_step(state, LaunchStep::Downloading);
    let target = strategy.target().clone();
    state.installations.insert(
        target.clone(),
        Installation::Downloading {
            strategy: strategy.clone(),
            percent: 0,
            attempt,
        },
    );
    Cmd::Multi(vec![
        fire_analytics_event(Event::DOWNLOAD_VERSION {
            version: target.to_string(),
        }),
        Cmd::DownloadVersion(strategy),
    ])
}

fn on_download_progressed(state: &mut State, strategy: &UpdateStrategy, percent: Percent) -> Cmd {
    if let Some(Installation::Downloading {
        strategy: current,
        percent: progress,
        ..
    }) = state.installations.get_mut(strategy.target())
    {
        if current == strategy {
            *progress = percent;
        }
    }
    Cmd::None
}

fn on_version_download_completed(state: &mut State, strategy: UpdateStrategy) -> Cmd {
    apply_step(state, LaunchStep::Installing);
    let target = strategy.target().clone();
    let attempt = installation_update_attempt(state, &target);
    state.installations.insert(
        target.clone(),
        Installation::Installing {
            strategy: strategy.clone(),
            attempt,
        },
    );
    let version = target.to_string();
    Cmd::Multi(vec![
        fire_analytics_event(Event::DOWNLOAD_VERSION_SUCCESS {
            version: version.clone(),
        }),
        fire_analytics_event(Event::INSTALL_VERSION_START { version }),
        Cmd::InstallVersion(strategy),
    ])
}

fn on_version_download_failed(state: &mut State, strategy: UpdateStrategy, error: DCLError) -> Cmd {
    let failure = fire_analytics_event(Event::DOWNLOAD_VERSION_ERROR {
        version: Some(strategy.target().to_string()),
        error: error.to_string(),
    });
    Cmd::Multi(vec![
        failure,
        fall_back_to_zip_or_retry_download(state, strategy, Arc::new(error), "download"),
    ])
}

fn on_version_installation_failed(
    state: &mut State,
    strategy: UpdateStrategy,
    error: DCLError,
) -> Cmd {
    let failure = fire_analytics_event(Event::INSTALL_VERSION_ERROR {
        version: Some(strategy.target().to_string()),
        error: error.to_string(),
    });
    Cmd::Multi(vec![
        failure,
        fall_back_to_zip_or_retry_download(state, strategy, Arc::new(error), "install"),
    ])
}

/// A delta failure falls back to the zip within the same launch; a zip failure is an attempt.
fn fall_back_to_zip_or_retry_download(
    state: &mut State,
    strategy: UpdateStrategy,
    error: Arc<DCLError>,
    phase: &str,
) -> Cmd {
    let target = strategy.target().clone();
    match strategy {
        UpdateStrategy::ByDelta { .. } => Cmd::Multi(vec![
            fire_analytics_event(Event::UPDATE_STRATEGY_FALLBACK {
                version: target.to_string(),
                phase: phase.to_owned(),
                error: error.to_string(),
            }),
            report_final_error_to_sentry(error),
            enter_downloading_step(state, UpdateStrategy::ByZip { target }, 1),
        ]),
        UpdateStrategy::ByZip { .. } => {
            let attempt = installation_update_attempt(state, &target);
            let report = report_attempt_error(&error, attempt);
            if is_final_attempt(attempt, &error) {
                state
                    .installations
                    .insert(target, Installation::Failed { error, attempt });
                return report;
            }
            let retry = enter_downloading_step(state, strategy, attempt.saturating_add(1));
            Cmd::Multi(vec![report, retry])
        }
    }
}

fn installation_update_attempt(state: &State, version: &Version) -> Attempt {
    match state.installations.get(version) {
        Some(
            Installation::Downloading { attempt, .. } | Installation::Installing { attempt, .. },
        ) => *attempt,
        _ => 1,
    }
}

fn on_version_installation_completed(
    state: &mut State,
    version: Version,
    ready: ReadyInstallation,
) -> Cmd {
    state
        .installations
        .insert(version.clone(), Installation::Ready(ready));
    let success = fire_analytics_event(Event::INSTALL_VERSION_SUCCESS {
        version: version.to_string(),
    });
    let is_target = latest_fetched_version(state).is_some_and(|target| target == version);
    if is_target && matches!(launch_step(state), Some(LaunchStep::Installing)) {
        return Cmd::Multi(vec![success, enter_launching_step(state, version)]);
    }
    success
}

// Launch

fn enter_launching_step(state: &mut State, version: Version) -> Cmd {
    let deeplink = intended_deeplink(state);
    apply_step(state, LaunchStep::Launching);
    Cmd::LaunchVersion(version, deeplink)
}

fn on_version_launch_failed(state: &mut State, error: DCLError) -> Cmd {
    let error = Arc::new(error);
    apply_step(
        state,
        LaunchStep::Failed {
            error: error.clone(),
            attempt: 1,
        },
    );
    report_final_error_to_sentry(error)
}

// Attempts and reporting

/// A deeplink consume-wait timeout cannot be helped by retrying; everything else is retried.
const fn is_error_retryable(error: &DCLError) -> bool {
    !matches!(error, DCLError::E3001_OPEN_DEEPLINK_TIMEOUT)
}

const fn is_final_attempt(attempt: Attempt, error: &DCLError) -> bool {
    attempt >= SILENT_ATTEMPTS || !is_error_retryable(error)
}

fn report_attempt_error(error: &Arc<DCLError>, attempt: Attempt) -> Cmd {
    Cmd::Multi(vec![
        fire_analytics_event(Event::FLOW_ATTEMPT_ERROR {
            message: error.to_string(),
            attempt,
        }),
        Cmd::ReportToSentry {
            error: error.clone(),
            attempt,
            is_final: is_final_attempt(attempt, error),
        },
    ])
}

const fn report_final_error_to_sentry(error: Arc<DCLError>) -> Cmd {
    Cmd::ReportToSentry {
        error,
        attempt: 1,
        is_final: true,
    }
}

const fn fire_analytics_event(event: Event) -> Cmd {
    Cmd::FireAnalyticsEvent(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::state::Config;

    fn version(value: &str) -> Version {
        Version::parse(value).unwrap_or(Version::Dev)
    }

    fn deeplink() -> DeepLink {
        DeepLink::from_query("position=0,0")
    }

    fn flat(cmd: Cmd) -> Vec<Cmd> {
        match cmd {
            Cmd::None => vec![],
            Cmd::Multi(cmds) => cmds.into_iter().flat_map(flat).collect(),
            cmd => vec![cmd],
        }
    }

    fn step_msg(state: State, msg: Msg) -> (State, Vec<Cmd>) {
        let (state, cmd) = update(state, msg);
        (state, flat(cmd))
    }

    fn has(cmds: &[Cmd], predicate: impl Fn(&Cmd) -> bool) -> bool {
        cmds.iter().any(predicate)
    }

    fn at(state: &State, predicate: impl Fn(&LaunchStep) -> bool) -> bool {
        launch_step(state).is_some_and(predicate)
    }

    fn is_final_report(cmd: &Cmd) -> bool {
        matches!(cmd, Cmd::ReportToSentry { is_final: true, .. })
    }

    fn is_breadcrumb(cmd: &Cmd) -> bool {
        matches!(
            cmd,
            Cmd::ReportToSentry {
                is_final: false,
                ..
            }
        )
    }

    fn delta_state() -> State {
        State::new(Config {
            use_delta_mode: true,
            ..Config::default()
        })
    }

    /// Intention received and the scan answered: the latest version is being fetched.
    fn fetching(state: State, ready: Vec<ReadyInstallation>) -> State {
        let (state, cmds) = step_msg(state, Msg::LaunchIntentionReceived(None));
        assert!(has(&cmds, |c| matches!(c, Cmd::EnsureLocalInstallations)));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::Fetch(FetchRequest::LatestVersion)
        )));
        let (state, _) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(ready));
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Fetching { scanned: true }
        )));
        assert!(matches!(
            state.latest_version,
            NetworkResource::Fetching { attempt: 1 }
        ));
        state
    }

    fn fetched(state: State, latest: &str) -> (State, Vec<Cmd>) {
        step_msg(state, Msg::LatestVersionFetched(Ok(version(latest))))
    }

    fn fetch_failed(state: State) -> (State, Vec<Cmd>) {
        step_msg(
            state,
            Msg::LatestVersionFetched(Err(DCLError::E3003_CANT_GET_VERSION)),
        )
    }

    fn manifest_failed(state: State) -> (State, Vec<Cmd>) {
        step_msg(
            state,
            Msg::DeltaManifestFetched(Err(DCLError::E3003_CANT_GET_VERSION)),
        )
    }

    fn zip_download_failed(state: State, target: &str) -> (State, Vec<Cmd>) {
        let strategy = UpdateStrategy::ByZip {
            target: version(target),
        };
        step_msg(
            state,
            Msg::VersionDownloadFailed(strategy, DCLError::E2006_DOWNLOAD_FAILED_NETWORK_TIMEOUT),
        )
    }

    #[test]
    fn intention_without_deeplink_scans_and_fetches_at_once() {
        let (state, cmds) = step_msg(State::default(), Msg::LaunchIntentionReceived(None));
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Fetching { scanned: false }
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::EnsureLocalInstallations)));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::Fetch(FetchRequest::LatestVersion)
        )));
        assert!(!has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
    }

    #[test]
    fn intention_with_deeplink_checks_running_instances_first() {
        let (state, cmds) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::DeeplinkOpening { attempt: 1 }
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
        assert!(!has(&cmds, |c| matches!(c, Cmd::Fetch(_))));
    }

    #[test]
    fn running_instance_hands_the_deeplink_off() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, cmds) = step_msg(state, Msg::RunningInstancesChecked { any: true });
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::DeeplinkOpening { .. }
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));

        let (state, cmds) = step_msg(state, Msg::DeeplinkHandedOff);
        assert!(at(&state, |s| matches!(s, LaunchStep::DeepLinkPass)));
        assert!(cmds.is_empty());
    }

    #[test]
    fn no_running_instance_continues_with_the_fetch() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, cmds) = step_msg(state, Msg::RunningInstancesChecked { any: false });
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Fetching { scanned: false }
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::EnsureLocalInstallations)));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::Fetch(FetchRequest::LatestVersion)
        )));
    }

    #[test]
    fn handoff_timeout_is_final_on_the_first_attempt() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(state, Msg::RunningInstancesChecked { any: true });
        let (state, cmds) = step_msg(
            state,
            Msg::DeeplinkHandoffFailed(DCLError::E3001_OPEN_DEEPLINK_TIMEOUT),
        );
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Failed { attempt: 1, .. }
        )));
        assert!(has(&cmds, is_final_report));
        assert!(!has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));
    }

    #[test]
    fn handoff_write_error_retries_silently() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(state, Msg::RunningInstancesChecked { any: true });
        let (state, cmds) = step_msg(
            state,
            Msg::DeeplinkHandoffFailed(DCLError::E3003_CANT_GET_VERSION),
        );
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::DeeplinkOpening { attempt: 2 }
        )));
        assert!(has(&cmds, is_breadcrumb));
        assert!(has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));
    }

    #[test]
    fn installed_latest_launches_with_the_deeplink() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(state, Msg::RunningInstancesChecked { any: false });
        let ready = vec![ReadyInstallation(version("v1.2.3"))];
        let (state, _) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(ready));
        let (state, cmds) = fetched(state, "v1.2.3");
        assert!(at(&state, |s| matches!(s, LaunchStep::Launching)));
        assert!(has(&cmds, |c| matches!(c, Cmd::LaunchVersion(_, Some(_)))));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::INSTALL_VERSION_SKIPPED { .. })
        )));

        let (state, _) = step_msg(state, Msg::VersionLaunched(version("v1.2.3")));
        assert!(at(&state, |s| matches!(s, LaunchStep::Launched)));
    }

    #[test]
    fn latest_before_scan_waits_for_the_scan() {
        let (state, _) = step_msg(State::default(), Msg::LaunchIntentionReceived(None));
        let (state, cmds) = fetched(state, "v2.0.0");
        assert!(!has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Fetching { scanned: false }
        )));

        let (state, cmds) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(vec![]));
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));
    }

    #[test]
    fn fresh_install_downloads_the_zip_without_a_manifest() {
        let state = fetching(delta_state(), vec![]);
        let (state, cmds) = fetched(state, "v2.0.0");
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Downloading {
                strategy: UpdateStrategy::ByZip { .. },
                percent: 0,
                attempt: 1
            })
        ));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::UPDATE_STRATEGY_DECIDED { leaf, .. }) if leaf == "fresh_install"
        )));
        assert!(!has(&cmds, |c| matches!(
            c,
            Cmd::Fetch(FetchRequest::DeltaManifest { .. })
        )));
    }

    #[test]
    fn delta_disabled_downloads_the_zip() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(State::default(), ready);
        let (_, cmds) = fetched(state, "v2.0.0");
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::UPDATE_STRATEGY_DECIDED { leaf, .. }) if leaf == "delta_disabled"
        )));
    }

    #[test]
    fn installed_base_fetches_the_manifest_then_goes_delta() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(delta_state(), ready);
        let (state, cmds) = fetched(state, "v2.0.0");
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::Fetch(FetchRequest::DeltaManifest { current_version }) if *current_version == version("v1.0.0")
        )));
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Fetching { scanned: true }
        )));
        assert!(matches!(
            state.delta_manifest,
            NetworkResource::Fetching { attempt: 1 }
        ));

        let (state, cmds) = step_msg(state, Msg::DeltaManifestFetched(Ok(Some(DeltaManifest))));
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::DownloadVersion(UpdateStrategy::ByDelta { from, .. }) if *from == version("v1.0.0")
        )));
    }

    #[test]
    fn missing_manifest_means_zip() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(delta_state(), ready);
        let (state, _) = fetched(state, "v2.0.0");
        let (_, cmds) = step_msg(state, Msg::DeltaManifestFetched(Ok(None)));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::UPDATE_STRATEGY_DECIDED { leaf, .. }) if leaf == "manifest_missing"
        )));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::DownloadVersion(UpdateStrategy::ByZip { .. })
        )));
    }

    #[test]
    fn manifest_errors_retry_silently_then_fall_back_to_zip() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(delta_state(), ready);
        let (mut state, _) = fetched(state, "v2.0.0");
        for attempt in 1..SILENT_ATTEMPTS {
            let (next, cmds) = manifest_failed(state);
            assert!(has(&cmds, is_breadcrumb));
            assert!(has(&cmds, |c| matches!(
                c,
                Cmd::Fetch(FetchRequest::DeltaManifest { .. })
            )));
            assert!(matches!(
                next.delta_manifest,
                NetworkResource::Fetching { attempt: a } if a == attempt.saturating_add(1)
            ));
            state = next;
        }
        let (state, cmds) = manifest_failed(state);
        assert!(matches!(
            state.delta_manifest,
            NetworkResource::Error { .. }
        ));
        assert!(has(&cmds, is_final_report));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::UPDATE_STRATEGY_DECIDED { leaf, .. }) if leaf == "manifest_unavailable"
        )));
    }

    #[test]
    fn fetch_errors_retry_silently_then_stop() {
        let mut state = fetching(State::default(), vec![]);
        for attempt in 1..SILENT_ATTEMPTS {
            let (next, cmds) = fetch_failed(state);
            assert!(has(&cmds, |c| matches!(
                c,
                Cmd::FireAnalyticsEvent(Event::FLOW_ATTEMPT_ERROR { attempt: a, .. }) if *a == attempt
            )));
            assert!(has(&cmds, is_breadcrumb));
            assert!(has(&cmds, |c| matches!(
                c,
                Cmd::Fetch(FetchRequest::LatestVersion)
            )));
            state = next;
        }
        let (state, cmds) = fetch_failed(state);
        assert!(matches!(
            state.latest_version,
            NetworkResource::Error {
                attempt: SILENT_ATTEMPTS,
                ..
            }
        ));
        assert!(has(&cmds, is_final_report));
        assert!(!has(&cmds, |c| matches!(c, Cmd::Fetch(_))));
        assert!(is_waiting_for_user_retry(&state));
    }

    #[test]
    fn download_install_launch_chain() {
        let state = fetching(State::default(), vec![]);
        let (state, _) = fetched(state, "v2.0.0");
        let strategy = UpdateStrategy::ByZip {
            target: version("v2.0.0"),
        };

        let (state, cmds) = step_msg(state, Msg::DownloadProgressed(strategy.clone(), 40));
        assert!(cmds.is_empty());
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Downloading { percent: 40, .. })
        ));

        let (state, cmds) = step_msg(state, Msg::VersionDownloadCompleted(strategy));
        assert!(at(&state, |s| matches!(s, LaunchStep::Installing)));
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Installing { attempt: 1, .. })
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::InstallVersion(_))));

        let (state, cmds) = step_msg(
            state,
            Msg::VersionInstallationCompleted(
                version("v2.0.0"),
                ReadyInstallation(version("v2.0.0")),
            ),
        );
        assert!(at(&state, |s| matches!(s, LaunchStep::Launching)));
        assert!(has(&cmds, |c| matches!(c, Cmd::LaunchVersion(_, None))));
        assert!(!has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::INSTALL_VERSION_SKIPPED { .. })
        )));
    }

    #[test]
    fn zip_download_failure_retries_then_fails_the_installation() {
        let state = fetching(State::default(), vec![]);
        let (state, _) = fetched(state, "v2.0.0");
        let (state, cmds) = zip_download_failed(state, "v2.0.0");
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Downloading { attempt: 2, .. })
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));

        let (state, _) = zip_download_failed(state, "v2.0.0");
        let (state, cmds) = zip_download_failed(state, "v2.0.0");
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Failed {
                attempt: SILENT_ATTEMPTS,
                ..
            })
        ));
        assert!(has(&cmds, is_final_report));
        assert!(!has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));
        assert!(is_waiting_for_user_retry(&state));
    }

    #[test]
    fn zip_install_failure_downloads_again() {
        let state = fetching(State::default(), vec![]);
        let (state, _) = fetched(state, "v2.0.0");
        let strategy = UpdateStrategy::ByZip {
            target: version("v2.0.0"),
        };
        let (state, _) = step_msg(state, Msg::VersionDownloadCompleted(strategy.clone()));
        let (state, cmds) = step_msg(
            state,
            Msg::VersionInstallationFailed(strategy, DCLError::E3004_CANT_RENAME_LATEST),
        );
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Downloading { attempt: 2, .. })
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));
    }

    #[test]
    fn delta_failure_falls_back_to_zip_in_the_same_launch() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(delta_state(), ready);
        let (state, _) = fetched(state, "v2.0.0");
        let (state, _) = step_msg(state, Msg::DeltaManifestFetched(Ok(Some(DeltaManifest))));
        let strategy = UpdateStrategy::ByDelta {
            target: version("v2.0.0"),
            from: version("v1.0.0"),
            manifest: Arc::new(DeltaManifest),
        };
        let (state, _) = step_msg(state, Msg::VersionDownloadCompleted(strategy.clone()));
        let (state, cmds) = step_msg(
            state,
            Msg::VersionInstallationFailed(strategy, DCLError::E3004_CANT_RENAME_LATEST),
        );
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(matches!(
            state.installations.get(&version("v2.0.0")),
            Some(Installation::Downloading {
                strategy: UpdateStrategy::ByZip { .. },
                attempt: 1,
                ..
            })
        ));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::UPDATE_STRATEGY_FALLBACK { phase, .. }) if phase == "install"
        )));
        assert!(has(&cmds, is_final_report));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::DownloadVersion(UpdateStrategy::ByZip { .. })
        )));
    }

    #[test]
    fn launch_failure_is_final_and_reported() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(State::default(), ready);
        let (state, _) = fetched(state, "v1.0.0");
        let (state, cmds) = step_msg(
            state,
            Msg::VersionLaunchFailed(version("v1.0.0"), DCLError::E3003_CANT_GET_VERSION),
        );
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::Failed { attempt: 1, .. }
        )));
        assert!(has(&cmds, is_final_report));
    }

    #[test]
    fn retry_resets_and_restarts_with_the_same_deeplink() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(state, Msg::RunningInstancesChecked { any: false });
        let (mut state, _) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(vec![]));
        for _ in 0..SILENT_ATTEMPTS {
            let (next, _) = fetch_failed(state);
            state = next;
        }
        let (state, cmds) = step_msg(state, Msg::RetryIntentionReceived);
        assert!(matches!(state.latest_version, NetworkResource::Unknown));
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::DeeplinkOpening { attempt: 1 }
        )));
        assert!(super::intended_deeplink(&state).is_some());
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::RETRY_FLOW_BUTTON_CLICK { .. })
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
    }

    #[test]
    fn retry_is_ignored_unless_stuck() {
        let state = fetching(State::default(), vec![]);
        let (state, cmds) = step_msg(state, Msg::RetryIntentionReceived);
        assert!(matches!(cmds.as_slice(), [Cmd::Ignore(_)]));
        assert!(matches!(
            state.latest_version,
            NetworkResource::Fetching { attempt: 1 }
        ));
    }

    #[test]
    fn deeplink_after_launch_starts_over_with_the_instance_check() {
        let ready = vec![ReadyInstallation(version("v1.0.0"))];
        let state = fetching(State::default(), ready);
        let (state, _) = fetched(state, "v1.0.0");
        let (state, _) = step_msg(state, Msg::VersionLaunched(version("v1.0.0")));
        let (state, cmds) = step_msg(state, Msg::DeeplinkReceived(deeplink()));
        assert!(at(&state, |s| matches!(
            s,
            LaunchStep::DeeplinkOpening { .. }
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
    }

    #[test]
    fn deeplink_mid_flow_only_replaces_the_deeplink() {
        let state = fetching(State::default(), vec![]);
        let (state, _) = fetched(state, "v2.0.0");
        let (state, cmds) = step_msg(state, Msg::DeeplinkReceived(deeplink()));
        assert!(matches!(cmds.as_slice(), [Cmd::Ignore(_)]));
        assert!(at(&state, |s| matches!(s, LaunchStep::Downloading)));
        assert!(super::intended_deeplink(&state).is_some());
    }
}
