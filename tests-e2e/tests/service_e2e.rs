#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::todo,
    clippy::dbg_macro
)]
#![allow(clippy::uninlined_format_args, clippy::future_not_send)]

//! Layer 1 e2e: the real debug `dcl_launcher_service`, driven over the real
//! protocol, against a mock CDN and a stub Explorer. All tests are `#[ignore]`
//! so plain `cargo test` (pre-commit) skips them; run via `scripts/run-e2e.rs`
//! or `cargo test -- --include-ignored`.

use std::time::Duration;

use anyhow::{Context, Result};
use dcl_launcher_ipc::protocol::{Command, ServiceState, ShutdownReason};
use dcl_launcher_shared::types::{Status, Step};
use dcl_launcher_tests_e2e::harness::{test_args, TestEnv};

const STUB: &str = env!("CARGO_BIN_EXE_stub_explorer");
const LONG: Duration = Duration::from_secs(90);

fn env() -> Result<TestEnv> {
    TestEnv::new(STUB)
}

async fn within<T>(fut: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(LONG, fut)
        .await
        .context("e2e step timed out")?
}

fn has_step(events: &[Status], pred: impl Fn(&Step) -> bool) -> bool {
    events.iter().any(|s| match s {
        Status::State { step } => pred(step),
        Status::Error { .. } => false,
    })
}

fn has_error(events: &[Status]) -> bool {
    events.iter().any(|s| matches!(s, Status::Error { .. }))
}

fn argv_of(launch: &serde_json::Value) -> Vec<String> {
    launch
        .get("argv")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn pid_of(pidfile: &serde_json::Value) -> Option<u64> {
    pidfile.get("pid").and_then(serde_json::Value::as_u64)
}

// ---- scenarios -------------------------------------------------------------

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn normal_launch_streams_status_and_launches_stub() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut client, service_version) = within(env.client()).await?;
    assert!(!service_version.is_empty());

    let (response, events) = within(env.launch_collect(&mut client, None)).await?;
    assert!(response.ok, "launch failed: {:?}", response.user_message);

    assert!(has_step(&events, |s| matches!(s, Step::Fetching)));
    assert!(has_step(&events, |s| matches!(s, Step::Downloading { .. })));
    assert!(has_step(&events, |s| matches!(s, Step::Installing { .. })));
    assert!(has_step(&events, |s| matches!(s, Step::Launching)));
    assert!(!has_error(&events));

    let launches = env.wait_stub_launches(1, Duration::from_secs(10)).await?;
    let argv = argv_of(launches.first().context("no launch record")?);
    assert!(argv.contains(&"--session_id".to_owned()), "argv: {argv:?}");
    assert!(
        argv.contains(&"--launcher_anonymous_id".to_owned()),
        "argv: {argv:?}"
    );
    assert!(argv.contains(&"--provider".to_owned()), "argv: {argv:?}");

    // The service outlives its clients.
    drop(client);
    let (mut again, _) = within(env.client()).await?;
    let state = within(env.view_state(&mut again)).await?;
    assert_eq!(state, ServiceState::Idle);

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn second_launch_skips_download() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;

    let (first, _) = within(env.launch_collect(&mut client, None)).await?;
    assert!(first.ok);
    env.wait_stub_launches(1, Duration::from_secs(10)).await?;

    let (second, events) = within(env.launch_collect(&mut client, None)).await?;
    assert!(second.ok, "relaunch failed: {:?}", second.user_message);
    assert!(
        !has_step(&events, |s| matches!(s, Step::Downloading { .. })),
        "already-installed launch must not download"
    );
    env.wait_stub_launches(2, Duration::from_secs(10)).await?;

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn running_service_is_reused() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (_c1, _) = within(env.client()).await?;
    let pid_first = pid_of(&env.pidfile().context("no pidfile")?);

    let (_c2, _) = within(env.client()).await?;
    let pid_second = pid_of(&env.pidfile().context("no pidfile")?);
    assert!(pid_first.is_some());
    assert_eq!(pid_first, pid_second);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn second_service_instance_exits_immediately() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;

    env.spawn_service()?;
    let code = env.wait_last_child_exit(Duration::from_secs(15))?;
    assert_eq!(code, 0, "the loser of the bind race must exit 0");

    let state = within(env.view_state(&mut client)).await?;
    assert_eq!(state, ServiceState::Idle);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn flow_error_surfaces_and_retry_recovers() -> Result<()> {
    let env = env()?;
    env.cdn.configure(|s| s.latest_status = Some(500));
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;

    let (response, events) = within(env.launch_collect(&mut client, None)).await?;
    assert!(!response.ok);
    assert!(
        response
            .user_message
            .as_deref()
            .is_some_and(|m| !m.is_empty()),
        "error must carry a user message"
    );
    assert!(has_error(&events));

    let state = within(env.view_state(&mut client)).await?;
    assert!(
        matches!(state, ServiceState::Errored { .. }),
        "state after failure: {state:?}"
    );

    env.cdn.configure(|s| s.latest_status = None);
    let retry = within(client.request(Command::Retry { args: test_args() }, |_| {})).await?;
    assert!(retry.ok, "retry failed: {:?}", retry.user_message);
    env.wait_stub_launches(1, Duration::from_secs(10)).await?;

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn concurrent_launches_join_one_flow() -> Result<()> {
    let env = env()?;
    env.cdn
        .configure(|s| s.zip_chunk_delay = Some(Duration::from_millis(30)));
    env.spawn_service()?;

    let (mut c1, _) = within(env.client()).await?;
    let (mut c2, _) = within(env.client()).await?;

    let first = tokio::spawn(async move {
        let mut events = Vec::new();
        let response = c1
            .request(Command::Launch { deeplink: None, args: test_args() }, |s| events.push(s))
            .await;
        (response, events)
    });

    tokio::time::sleep(Duration::from_millis(700)).await;
    let (second, _) = within(env.launch_collect(&mut c2, None)).await?;
    assert!(second.ok, "joined launch failed: {:?}", second.user_message);

    let (first_response, _) = first.await?;
    assert!(first_response?.ok);

    // Both requests resolved, but exactly one flow ran.
    let launches = env.wait_stub_launches(1, Duration::from_secs(10)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(env.stub_launches().len(), launches.len());
    assert_eq!(launches.len(), 1);
    assert_eq!(env.cdn.snapshot(|s| s.zip_hits), 1);

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn cold_deeplink_reaches_stub_argv() -> Result<()> {
    let deeplink = "decentraland://open?position=0%2C0".to_owned();
    let env = env()?;
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;

    let (response, _) = within(env.launch_collect(&mut client, Some(deeplink.clone()))).await?;
    assert!(response.ok);

    let launches = env.wait_stub_launches(1, Duration::from_secs(10)).await?;
    let argv = argv_of(launches.first().context("no launch record")?);
    assert_eq!(argv.get(1), Some(&deeplink), "argv: {argv:?}");

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn deeplink_passthrough_bridge_consumed_then_ignored() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;

    let (first, _) = within(env.launch_collect(&mut client, None)).await?;
    assert!(first.ok);
    env.wait_stub_launches(1, Duration::from_secs(10)).await?;

    // Explorer runs -> the deeplink goes through the bridge file.
    let deeplink = "decentraland://open?position=1%2C1".to_owned();
    let (via_bridge, events) =
        within(env.launch_collect(&mut client, Some(deeplink.clone()))).await?;
    assert!(
        via_bridge.ok,
        "passthrough failed: {:?}",
        via_bridge.user_message
    );
    assert!(has_step(&events, |s| matches!(s, Step::DeeplinkOpening)));
    assert_eq!(env.stub_launches().len(), 1, "no second Explorer instance");
    let bridge = env.bridge_log();
    assert!(
        bridge
            .iter()
            .any(|v| v["deeplink"].as_str() == Some(deeplink.as_str())),
        "bridge log: {bridge:?}"
    );

    // The stub stops consuming -> the 3s bridge timeout fires.
    env.set_ignore_bridge(true);
    let deeplink2 = "decentraland://open?position=2%2C2".to_owned();
    let (ignored, _) = within(env.launch_collect(&mut client, Some(deeplink2))).await?;
    assert!(!ignored.ok, "unconsumed bridge must fail");
    assert_eq!(env.stub_launches().len(), 1);

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn inject_deeplink_mid_flow_applies_to_launch() -> Result<()> {
    let deeplink = "decentraland://open?position=3%2C3".to_owned();
    let env = env()?;
    env.cdn
        .configure(|s| s.zip_chunk_delay = Some(Duration::from_millis(30)));
    env.spawn_service()?;

    let (mut c1, _) = within(env.client()).await?;
    let launch =
        tokio::spawn(async move { c1.request(Command::Launch { deeplink: None, args: test_args() }, |_| {}).await });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let (mut c2, _) = within(env.client()).await?;
    let inject = c2
        .request_silent(Command::InjectDeeplink {
            url: deeplink.clone(),
        })
        .await?;
    assert!(inject.ok);

    assert!(launch.await??.ok);
    let launches = env.wait_stub_launches(1, Duration::from_secs(10)).await?;
    let argv = argv_of(launches.first().context("no launch record")?);
    assert!(argv.contains(&deeplink), "argv: {argv:?}");

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn view_current_state_tracks_the_flow() -> Result<()> {
    let env = env()?;
    env.cdn
        .configure(|s| s.zip_chunk_delay = Some(Duration::from_millis(30)));
    env.spawn_service()?;

    let (mut c1, _) = within(env.client()).await?;
    let (mut c2, _) = within(env.client()).await?;
    let state = within(env.view_state(&mut c2)).await?;
    assert_eq!(state, ServiceState::Idle);

    let launch =
        tokio::spawn(async move { c1.request(Command::Launch { deeplink: None, args: test_args() }, |_| {}).await });

    let mut saw_busy = false;
    for _ in 0_u8..100 {
        let state = within(env.view_state(&mut c2)).await?;
        if matches!(state, ServiceState::Busy { .. }) {
            saw_busy = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(saw_busy, "never observed a busy state during the flow");

    assert!(launch.await??.ok);
    let state = within(env.view_state(&mut c2)).await?;
    assert_eq!(state, ServiceState::Idle);

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn shutdown_stops_service_and_removes_pidfile() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;
    assert!(env.pidfile().is_some());

    let response = client
        .request_silent(Command::Shutdown {
            reason: ShutdownReason::User,
        })
        .await?;
    assert!(response.ok);

    let code = env.wait_last_child_exit(Duration::from_secs(10))?;
    assert_eq!(code, 0);
    assert!(env.pidfile().is_none(), "pidfile must be removed");
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn client_disconnect_mid_download_does_not_stop_the_flow() -> Result<()> {
    let env = env()?;
    env.cdn
        .configure(|s| s.zip_chunk_delay = Some(Duration::from_millis(30)));
    env.spawn_service()?;

    let (mut c1, _) = within(env.client()).await?;
    let launch =
        tokio::spawn(async move { c1.request(Command::Launch { deeplink: None, args: test_args() }, |_| {}).await });

    let (mut c2, _) = within(env.client()).await?;
    let mut saw_busy = false;
    for _ in 0_u8..100 {
        if matches!(
            within(env.view_state(&mut c2)).await?,
            ServiceState::Busy { .. }
        ) {
            saw_busy = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(saw_busy);

    // The UI dies mid-download.
    launch.abort();
    let _ = launch.await;

    env.wait_stub_launches(1, Duration::from_secs(60)).await?;

    // The flow tail (launch step incl. the 3s early-death check) is still
    // running right after the stub appears — poll until it settles.
    let mut settled = false;
    for _ in 0_u8..100 {
        if within(env.view_state(&mut c2)).await? == ServiceState::Idle {
            settled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(settled, "the flow never settled back to idle");

    env.request_stub_exit(0);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn hello_and_notify_ui_closed_keep_service_healthy() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;

    let (mut c1, _) = within(env.client()).await?;
    let (mut c2, _) = within(env.client()).await?;

    let notified = c1.request_silent(Command::NotifyUiClosed).await?;
    assert!(notified.ok);
    drop(c1);

    let state = within(env.view_state(&mut c2)).await?;
    assert_eq!(state, ServiceState::Idle);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn malformed_frame_closes_only_that_connection() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut healthy, _) = within(env.client()).await?;

    within(send_garbage(&env)).await?;

    let state = within(env.view_state(&mut healthy)).await?;
    assert_eq!(state, ServiceState::Idle);
    Ok(())
}

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn install_is_blocked_while_explorer_runs() -> Result<()> {
    let env = env()?;
    env.spawn_service()?;
    let (mut client, _) = within(env.client()).await?;

    let (first, _) = within(env.launch_collect(&mut client, None)).await?;
    assert!(first.ok);
    env.wait_stub_launches(1, Duration::from_secs(10)).await?;

    // A newer version appears while the (stub) Explorer is still running.
    env.cdn.configure(|s| s.version = "v9.9.2".to_owned());
    let (blocked, _) = within(env.launch_collect(&mut client, None)).await?;
    assert!(!blocked.ok, "install over a running Explorer must fail");
    assert_eq!(env.stub_launches().len(), 1);

    env.request_stub_exit(0);
    Ok(())
}

// ---- raw-wire helper --------------------------------------------------------

#[cfg(windows)]
async fn send_garbage(env: &TestEnv) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let name = format!(r"\\.\pipe\dcl-launcher-service-{}", env.endpoint);
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new().open(name)?;
    stream.write_all(b"this is not json\n").await?;
    let mut buf = [0_u8; 256];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn send_garbage(env: &TestEnv) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let path = dcl_launcher_ipc::transport::socket_path_for(Some(&env.endpoint));
    let mut stream = tokio::net::UnixStream::connect(path).await?;
    stream.write_all(b"this is not json\n").await?;
    let mut buf = [0_u8; 256];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    Ok(())
}
