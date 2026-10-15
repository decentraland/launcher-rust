use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;

use crate::analytics::event::Event;
use crate::deeplink_bridge::should_use_deeplink_bridge;
use crate::delta_manifest::DeltaManifest;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::utils;
use crate::version::Version;

use super::cmd::{Cmd, FetchRequest};
use super::msg::Msg;
use super::state::{
    DeeplinkHandoff, ExplorerLaunch, LocalInstallations, NetworkResource, RunningInstances, State,
    UserIntention,
};
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
        Msg::RunningInstancesChecked { any, checked_at } => {
            on_running_instances_checked(&mut state, any, checked_at)
        }
        Msg::DeeplinkHandedOff => {
            state.deeplink_handoff = DeeplinkHandoff::HandedOff;
            Cmd::None
        }
        Msg::DeeplinkHandoffFailed(error) => on_deeplink_handoff_failed(&mut state, error),
        Msg::VersionInstallationCompleted(version, ready) => {
            on_version_installation_completed(&mut state, version, ready)
        }
        Msg::VersionInstallationFailed(strategy, error) => {
            on_version_installation_failed(&mut state, strategy, error)
        }
        Msg::VersionLaunched(version) => {
            state.launch = ExplorerLaunch::Launched(version);
            Cmd::None
        }
        Msg::VersionLaunchFailed(_, error) => on_version_launch_failed(&mut state, error),
    };
    (state, cmd)
}

// Intention

fn intended_deeplink(state: &State) -> Option<DeepLink> {
    match &state.user_intention {
        UserIntention::Launch(deeplink) => deeplink.clone(),
        UserIntention::Idle => None,
    }
}

fn latest_fetched_version(state: &State) -> Option<Version> {
    match &state.latest_version {
        NetworkResource::Fetched(version) => Some(version.clone()),
        _ => None,
    }
}

fn is_launch_in_progress(state: &State) -> bool {
    matches!(state.running_instances, RunningInstances::Checking)
        || matches!(state.deeplink_handoff, DeeplinkHandoff::HandingOff { .. })
        || matches!(state.local_installations, LocalInstallations::Scanning)
        || matches!(state.latest_version, NetworkResource::Fetching { .. })
        || matches!(state.delta_manifest, NetworkResource::Fetching { .. })
        || matches!(
            state.launch,
            ExplorerLaunch::AwaitingRunningInstancesCheck(_) | ExplorerLaunch::Launching(_)
        )
        || state
            .local_installations
            .scanned()
            .is_some_and(|installations| {
                installations.values().any(|installation| {
                    matches!(
                        installation,
                        Installation::Downloading { .. } | Installation::Installing { .. }
                    )
                })
            })
}

/// A final error sits in a resource the launch depends on; only the retry button moves on.
fn is_waiting_for_user_retry(state: &State) -> bool {
    matches!(state.deeplink_handoff, DeeplinkHandoff::Failed { .. })
        || matches!(state.latest_version, NetworkResource::Error { .. })
        || matches!(state.launch, ExplorerLaunch::Failed { .. })
        || latest_fetched_version(state).is_some_and(|target| {
            matches!(
                target_installation(state, &target),
                Some(Installation::Failed { .. })
            )
        })
}

fn target_installation<'a>(state: &'a State, target: &Version) -> Option<&'a Installation> {
    state
        .local_installations
        .scanned()
        .and_then(|installations| installations.get(target))
}

fn on_launch_intention_received(state: &mut State, deeplink: Option<DeepLink>) -> Cmd {
    if is_launch_in_progress(state) {
        return Cmd::Ignore("launch already in progress");
    }
    if is_waiting_for_user_retry(state) {
        return Cmd::Ignore("launch failed; only the retry button restarts it");
    }
    begin_launch_intention(state, deeplink)
}

fn on_deeplink_received(state: &mut State, deeplink: DeepLink) -> Cmd {
    if is_launch_in_progress(state) {
        state.user_intention = UserIntention::Launch(Some(deeplink));
        return Cmd::Ignore(
            "launch in progress continues; LaunchVersion will carry the new deeplink",
        );
    }
    begin_launch_intention(state, Some(deeplink))
}

fn on_retry_intention_received(state: &mut State) -> Cmd {
    if !is_waiting_for_user_retry(state) {
        return Cmd::Ignore("retry not awaited: nothing has failed");
    }
    state.latest_version = NetworkResource::Unknown;
    state.delta_manifest = NetworkResource::Unknown;
    state.local_installations = LocalInstallations::Unknown;
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
    state.user_intention = UserIntention::Launch(deeplink);
    state.deeplink_handoff = DeeplinkHandoff::Idle;
    state.launch = ExplorerLaunch::Idle;
    if has_deeplink {
        state.running_instances = RunningInstances::Checking;
        return Cmd::CheckRunningInstances;
    }
    scan_installations_and_fetch_latest_version(state)
}

// Deeplink handoff

/// The result is recorded whenever it arrives; it drives the handoff only when the launch asked for it.
fn on_running_instances_checked(state: &mut State, any: bool, checked_at: SystemTime) -> Cmd {
    let was_awaited = matches!(state.running_instances, RunningInstances::Checking);
    state.running_instances = RunningInstances::Checked { any, checked_at };
    if !was_awaited {
        return Cmd::Ignore("instances check recorded: no handoff decision is pending");
    }
    match intended_deeplink(state) {
        Some(deeplink) if should_use_deeplink_bridge(&deeplink, &state.config.args, any) => {
            state.launch = ExplorerLaunch::Idle;
            state.deeplink_handoff = DeeplinkHandoff::HandingOff { attempt: 1 };
            Cmd::HandoffDeeplink(deeplink)
        }
        _ => match state.launch.clone() {
            ExplorerLaunch::AwaitingRunningInstancesCheck(version) => {
                start_version_launch(state, version)
            }
            _ => scan_installations_and_fetch_latest_version(state),
        },
    }
}

fn on_deeplink_handoff_failed(state: &mut State, error: DCLError) -> Cmd {
    let DeeplinkHandoff::HandingOff { attempt } = state.deeplink_handoff else {
        return Cmd::Ignore("stale handoff failure: no handoff is in progress");
    };
    let error = Arc::new(error);
    let report = report_attempt_error(&error, attempt);
    if is_final_attempt(attempt, &error) {
        state.deeplink_handoff = DeeplinkHandoff::Failed { error, attempt };
        return report;
    }
    let Some(deeplink) = intended_deeplink(state) else {
        return report;
    };
    state.deeplink_handoff = DeeplinkHandoff::HandingOff {
        attempt: attempt.saturating_add(1),
    };
    Cmd::Multi(vec![report, Cmd::HandoffDeeplink(deeplink)])
}

// Local presence, latest version and the delta manifest

fn scan_installations_and_fetch_latest_version(state: &mut State) -> Cmd {
    state.local_installations = LocalInstallations::Scanning;
    if matches!(state.latest_version, NetworkResource::Fetched(_)) {
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
    let mut installations = match std::mem::take(&mut state.local_installations) {
        LocalInstallations::Scanned(installations) => installations,
        LocalInstallations::Unknown | LocalInstallations::Scanning => BTreeMap::new(),
    };
    installations.retain(|_, installation| !matches!(installation, Installation::Ready(_)));
    for installation in ready {
        installations.insert(installation.0.clone(), Installation::Ready(installation));
    }
    state.local_installations = LocalInstallations::Scanned(installations);
    launch_or_update_once_scanned_and_fetched(state)
}

/// Highest ready release; dev builds are never a delta base.
fn highest_ready_release(state: &State) -> Option<Version> {
    state
        .local_installations
        .scanned()?
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
            Cmd::Multi(vec![
                success,
                launch_or_update_once_scanned_and_fetched(state),
            ])
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
            launch_or_update_once_scanned_and_fetched(state)
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
                        launch_or_update_once_scanned_and_fetched(state),
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

fn launch_or_update_once_scanned_and_fetched(state: &mut State) -> Cmd {
    if state.local_installations.scanned().is_none() {
        return Cmd::Ignore("waiting for the local presence scan");
    }
    let Some(target) = latest_fetched_version(state) else {
        return Cmd::Ignore("waiting for the latest version");
    };
    match target_installation(state, &target) {
        Some(Installation::Ready(_)) => {
            if !matches!(state.launch, ExplorerLaunch::Idle) {
                return Cmd::Ignore("latest version is already launching or launched");
            }
            let version = target.to_string();
            Cmd::Multi(vec![
                fire_analytics_event(Event::DOWNLOAD_VERSION_SKIPPED {
                    version: version.clone(),
                }),
                fire_analytics_event(Event::INSTALL_VERSION_SKIPPED { version }),
                start_version_launch(state, target),
            ])
        }
        Some(Installation::Downloading { .. } | Installation::Installing { .. }) => {
            Cmd::Ignore("update of the latest version is already in progress")
        }
        Some(Installation::Failed { .. }) => {
            Cmd::Ignore("update of the latest version failed; waiting for the user retry")
        }
        None => match decide_update_strategy(state, &target) {
            UpdateDecision::Wait => Cmd::Ignore("waiting for the delta manifest"),
            UpdateDecision::FetchManifest(from) => request_delta_manifest_fetch(state, from, 1),
            UpdateDecision::Download(strategy, leaf) => Cmd::Multi(vec![
                fire_analytics_event(Event::UPDATE_STRATEGY_DECIDED {
                    version: target.to_string(),
                    strategy: strategy.name().to_owned(),
                    leaf: leaf.to_owned(),
                }),
                start_version_download(state, strategy, 1),
            ]),
        },
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

fn start_version_download(state: &mut State, strategy: UpdateStrategy, attempt: Attempt) -> Cmd {
    let Some(installations) = state.local_installations.scanned_mut() else {
        return Cmd::Ignore("download requested before the local presence scan");
    };
    // An Explorer may start while the download runs; the launch checks again.
    state.running_instances = RunningInstances::Unknown;
    let target = strategy.target().clone();
    installations.insert(
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
    }) = state
        .local_installations
        .scanned_mut()
        .and_then(|installations| installations.get_mut(strategy.target()))
    {
        if current == strategy {
            *progress = percent;
        }
    }
    Cmd::None
}

fn on_version_download_completed(state: &mut State, strategy: UpdateStrategy) -> Cmd {
    let target = strategy.target().clone();
    let attempt = installation_update_attempt(state, &target);
    let Some(installations) = state.local_installations.scanned_mut() else {
        return Cmd::Ignore("download completed before the local presence scan");
    };
    installations.insert(
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
            start_version_download(state, UpdateStrategy::ByZip { target }, 1),
        ]),
        UpdateStrategy::ByZip { .. } => {
            let attempt = installation_update_attempt(state, &target);
            let report = report_attempt_error(&error, attempt);
            if !is_final_attempt(attempt, &error) {
                let retry = start_version_download(state, strategy, attempt.saturating_add(1));
                return Cmd::Multi(vec![report, retry]);
            }
            if let Some(installations) = state.local_installations.scanned_mut() {
                installations.insert(target, Installation::Failed { error, attempt });
            }
            report
        }
    }
}

fn installation_update_attempt(state: &State, version: &Version) -> Attempt {
    match target_installation(state, version) {
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
    let Some(installations) = state.local_installations.scanned_mut() else {
        return Cmd::Ignore("installation completed before the local presence scan");
    };
    installations.insert(version.clone(), Installation::Ready(ready));
    let success = fire_analytics_event(Event::INSTALL_VERSION_SUCCESS {
        version: version.to_string(),
    });
    let is_target = latest_fetched_version(state).is_some_and(|target| target == version);
    if is_target && matches!(state.launch, ExplorerLaunch::Idle) {
        return Cmd::Multi(vec![success, start_version_launch(state, version)]);
    }
    success
}

// Launch

fn start_version_launch(state: &mut State, version: Version) -> Cmd {
    let deeplink = intended_deeplink(state);
    let check_is_current = matches!(state.running_instances, RunningInstances::Checked { .. });
    if deeplink.is_some() && !check_is_current {
        state.launch = ExplorerLaunch::AwaitingRunningInstancesCheck(version);
        state.running_instances = RunningInstances::Checking;
        return Cmd::CheckRunningInstances;
    }
    state.launch = ExplorerLaunch::Launching(version.clone());
    Cmd::LaunchVersion(version, deeplink)
}

fn on_version_launch_failed(state: &mut State, error: DCLError) -> Cmd {
    let error = Arc::new(error);
    state.launch = ExplorerLaunch::Failed {
        error: error.clone(),
    };
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

    fn installation<'a>(state: &'a State, target: &str) -> Option<&'a Installation> {
        target_installation(state, &version(target))
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
        assert!(state.local_installations.scanned().is_some());
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
        assert!(matches!(
            state.local_installations,
            LocalInstallations::Scanning
        ));
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::Idle));
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
        assert!(matches!(
            state.running_instances,
            RunningInstances::Checking
        ));
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::Idle));
        assert!(matches!(
            state.local_installations,
            LocalInstallations::Unknown
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
        assert!(!has(&cmds, |c| matches!(c, Cmd::Fetch(_))));
    }

    #[test]
    fn running_instance_hands_the_deeplink_off() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, cmds) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: true,
                checked_at: SystemTime::now(),
            },
        );
        assert!(matches!(
            state.deeplink_handoff,
            DeeplinkHandoff::HandingOff { attempt: 1 }
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));

        let (state, cmds) = step_msg(state, Msg::DeeplinkHandedOff);
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::HandedOff));
        assert!(cmds.is_empty());
    }

    #[test]
    fn no_running_instance_continues_with_the_fetch() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, cmds) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: false,
                checked_at: SystemTime::now(),
            },
        );
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::Idle));
        assert!(matches!(
            state.local_installations,
            LocalInstallations::Scanning
        ));
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
        let (state, _) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: true,
                checked_at: SystemTime::now(),
            },
        );
        let (state, cmds) = step_msg(
            state,
            Msg::DeeplinkHandoffFailed(DCLError::E3001_OPEN_DEEPLINK_TIMEOUT),
        );
        assert!(matches!(
            state.deeplink_handoff,
            DeeplinkHandoff::Failed { attempt: 1, .. }
        ));
        assert!(has(&cmds, is_final_report));
        assert!(!has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));
        assert!(is_waiting_for_user_retry(&state));
    }

    #[test]
    fn handoff_write_error_retries_silently() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: true,
                checked_at: SystemTime::now(),
            },
        );
        let (state, cmds) = step_msg(
            state,
            Msg::DeeplinkHandoffFailed(DCLError::E3003_CANT_GET_VERSION),
        );
        assert!(matches!(
            state.deeplink_handoff,
            DeeplinkHandoff::HandingOff { attempt: 2 }
        ));
        assert!(has(&cmds, is_breadcrumb));
        assert!(has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));
    }

    #[test]
    fn installed_latest_launches_with_the_deeplink() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: false,
                checked_at: SystemTime::now(),
            },
        );
        let ready = vec![ReadyInstallation(version("v1.2.3"))];
        let (state, _) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(ready));
        let (state, cmds) = fetched(state, "v1.2.3");
        assert!(matches!(state.launch, ExplorerLaunch::Launching(_)));
        assert!(has(&cmds, |c| matches!(c, Cmd::LaunchVersion(_, Some(_)))));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::INSTALL_VERSION_SKIPPED { .. })
        )));

        let (state, _) = step_msg(state, Msg::VersionLaunched(version("v1.2.3")));
        assert!(matches!(state.launch, ExplorerLaunch::Launched(_)));
    }

    #[test]
    fn latest_before_scan_waits_for_the_scan() {
        let (state, _) = step_msg(State::default(), Msg::LaunchIntentionReceived(None));
        let (state, cmds) = fetched(state, "v2.0.0");
        assert!(!has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));
        assert!(has(&cmds, |c| matches!(c, Cmd::Ignore(_))));
        assert!(matches!(
            state.local_installations,
            LocalInstallations::Scanning
        ));

        let (state, cmds) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(vec![]));
        assert!(matches!(
            installation(&state, "v2.0.0"),
            Some(Installation::Downloading { .. })
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));
    }

    #[test]
    fn fresh_install_downloads_the_zip_without_a_manifest() {
        let state = fetching(delta_state(), vec![]);
        let (state, cmds) = fetched(state, "v2.0.0");
        assert!(matches!(
            installation(&state, "v2.0.0"),
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
        assert!(matches!(
            state.delta_manifest,
            NetworkResource::Fetching { attempt: 1 }
        ));
        assert!(installation(&state, "v2.0.0").is_none());

        let (state, cmds) = step_msg(state, Msg::DeltaManifestFetched(Ok(Some(DeltaManifest))));
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::DownloadVersion(UpdateStrategy::ByDelta { from, .. }) if *from == version("v1.0.0")
        )));
        assert!(matches!(
            installation(&state, "v2.0.0"),
            Some(Installation::Downloading {
                strategy: UpdateStrategy::ByDelta { .. },
                ..
            })
        ));
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
        assert!(!is_waiting_for_user_retry(&state));
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
            installation(&state, "v2.0.0"),
            Some(Installation::Downloading { percent: 40, .. })
        ));

        let (state, cmds) = step_msg(state, Msg::VersionDownloadCompleted(strategy));
        assert!(matches!(
            installation(&state, "v2.0.0"),
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
        assert!(matches!(state.launch, ExplorerLaunch::Launching(_)));
        assert!(has(&cmds, |c| matches!(c, Cmd::LaunchVersion(_, None))));
        assert!(!has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::INSTALL_VERSION_SKIPPED { .. })
        )));
    }

    fn installed_with_deeplink(any_running_at_start: bool) -> State {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: any_running_at_start,
                checked_at: SystemTime::now(),
            },
        );
        let (state, _) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(vec![]));
        let (state, _) = fetched(state, "v2.0.0");
        assert!(matches!(state.running_instances, RunningInstances::Unknown));
        let strategy = UpdateStrategy::ByZip {
            target: version("v2.0.0"),
        };
        let (state, _) = step_msg(state, Msg::VersionDownloadCompleted(strategy));
        let (state, cmds) = step_msg(
            state,
            Msg::VersionInstallationCompleted(
                version("v2.0.0"),
                ReadyInstallation(version("v2.0.0")),
            ),
        );
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
        assert!(!has(&cmds, |c| matches!(c, Cmd::LaunchVersion(..))));
        assert!(matches!(
            state.launch,
            ExplorerLaunch::AwaitingRunningInstancesCheck(_)
        ));
        state
    }

    #[test]
    fn install_with_a_deeplink_rechecks_instances_before_launching() {
        let state = installed_with_deeplink(false);
        let (state, cmds) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: false,
                checked_at: SystemTime::now(),
            },
        );
        assert!(matches!(state.launch, ExplorerLaunch::Launching(_)));
        assert!(has(&cmds, |c| matches!(c, Cmd::LaunchVersion(_, Some(_)))));
        assert!(!has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::INSTALL_VERSION_SKIPPED { .. })
        )));
    }

    #[test]
    fn instance_appearing_during_the_download_takes_the_deeplink() {
        let state = installed_with_deeplink(false);
        let (state, cmds) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: true,
                checked_at: SystemTime::now(),
            },
        );
        assert!(matches!(state.launch, ExplorerLaunch::Idle));
        assert!(matches!(
            state.deeplink_handoff,
            DeeplinkHandoff::HandingOff { attempt: 1 }
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::HandoffDeeplink(_))));
        assert!(!has(&cmds, |c| matches!(c, Cmd::LaunchVersion(..))));
    }

    #[test]
    fn zip_download_failure_retries_then_fails_the_installation() {
        let state = fetching(State::default(), vec![]);
        let (state, _) = fetched(state, "v2.0.0");
        let (state, cmds) = zip_download_failed(state, "v2.0.0");
        assert!(matches!(
            installation(&state, "v2.0.0"),
            Some(Installation::Downloading { attempt: 2, .. })
        ));
        assert!(has(&cmds, |c| matches!(c, Cmd::DownloadVersion(_))));

        let (state, _) = zip_download_failed(state, "v2.0.0");
        let (state, cmds) = zip_download_failed(state, "v2.0.0");
        assert!(matches!(
            installation(&state, "v2.0.0"),
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
        assert!(matches!(
            installation(&state, "v2.0.0"),
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
        assert!(matches!(
            installation(&state, "v2.0.0"),
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
        assert!(matches!(state.launch, ExplorerLaunch::Failed { .. }));
        assert!(has(&cmds, is_final_report));
        assert!(is_waiting_for_user_retry(&state));
    }

    #[test]
    fn retry_resets_and_restarts_with_the_same_deeplink() {
        let (state, _) = step_msg(
            State::default(),
            Msg::LaunchIntentionReceived(Some(deeplink())),
        );
        let (state, _) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: false,
                checked_at: SystemTime::now(),
            },
        );
        let (mut state, _) = step_msg(state, Msg::LocalInstallationPresenceConfirmed(vec![]));
        for _ in 0..SILENT_ATTEMPTS {
            let (next, _) = fetch_failed(state);
            state = next;
        }
        let (state, cmds) = step_msg(state, Msg::RetryIntentionReceived);
        assert!(matches!(state.latest_version, NetworkResource::Unknown));
        assert!(matches!(
            state.local_installations,
            LocalInstallations::Unknown
        ));
        assert!(matches!(
            state.running_instances,
            RunningInstances::Checking
        ));
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::Idle));
        assert!(intended_deeplink(&state).is_some());
        assert!(has(&cmds, |c| matches!(
            c,
            Cmd::FireAnalyticsEvent(Event::RETRY_FLOW_BUTTON_CLICK { .. })
        )));
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
    }

    #[test]
    fn retry_is_ignored_unless_awaited() {
        let state = fetching(State::default(), vec![]);
        let (state, cmds) = step_msg(state, Msg::RetryIntentionReceived);
        assert!(matches!(cmds.as_slice(), [Cmd::Ignore(_)]));
        assert!(matches!(
            state.latest_version,
            NetworkResource::Fetching { attempt: 1 }
        ));
    }

    #[test]
    fn second_intention_during_a_launch_is_ignored() {
        let state = fetching(State::default(), vec![]);
        let (state, cmds) = step_msg(state, Msg::LaunchIntentionReceived(None));
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
        assert!(matches!(
            state.running_instances,
            RunningInstances::Checking
        ));
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::Idle));
        assert!(matches!(state.launch, ExplorerLaunch::Idle));
        assert!(has(&cmds, |c| matches!(c, Cmd::CheckRunningInstances)));
    }

    #[test]
    fn deeplink_mid_flow_only_replaces_the_deeplink() {
        let state = fetching(State::default(), vec![]);
        let (state, _) = fetched(state, "v2.0.0");
        let (state, cmds) = step_msg(state, Msg::DeeplinkReceived(deeplink()));
        assert!(matches!(cmds.as_slice(), [Cmd::Ignore(_)]));
        assert!(matches!(
            installation(&state, "v2.0.0"),
            Some(Installation::Downloading { .. })
        ));
        assert!(intended_deeplink(&state).is_some());
    }

    #[test]
    fn unrequested_instances_check_is_recorded_but_decides_nothing() {
        let state = fetching(State::default(), vec![]);
        let (state, cmds) = step_msg(
            state,
            Msg::RunningInstancesChecked {
                any: true,
                checked_at: SystemTime::now(),
            },
        );
        assert!(matches!(cmds.as_slice(), [Cmd::Ignore(_)]));
        assert!(matches!(
            state.running_instances,
            RunningInstances::Checked { any: true, .. }
        ));
        assert!(matches!(state.deeplink_handoff, DeeplinkHandoff::Idle));
    }
}
