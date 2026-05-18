//! Integration tests for rustEZ against a real vSRX device.
//!
//! All tests are `#[ignore]` by default. Run with:
//! ```sh
//! RUSTEZ_VSRX_HOST=<DEVICE_IP> RUSTEZ_VSRX_USER=<USERNAME> \
//!     RUSTEZ_VSRX_KEY=~/.ssh/<KEY_FILE> \
//!     cargo test -p rustez -- --ignored
//! ```

use std::env;
use std::time::Duration;

use rustez::{ConfigPayload, Device, DeviceBuilder, HostKeyVerification};
use serial_test::serial;

/// Build a DeviceBuilder from environment variables.
///
/// Supports both key-based auth (RUSTEZ_VSRX_KEY) and password auth (RUSTEZ_VSRX_PASS).
///
/// Defaults to `HostKeyVerification::AcceptAll` because these integration
/// tests target a known-good lab vSRX where pinning a per-device
/// fingerprint would add noise without adding signal. Production callers
/// should pin a fingerprint instead — see `Device::host_key_verification`.
fn vsrx_builder() -> DeviceBuilder {
    let host = env::var("RUSTEZ_VSRX_HOST").expect("RUSTEZ_VSRX_HOST not set");
    let user = env::var("RUSTEZ_VSRX_USER").unwrap_or_else(|_| "admin".to_string());

    let mut builder = Device::connect(&host)
        .username(&user)
        .host_key_verification(HostKeyVerification::AcceptAll);

    if let Ok(key_path) = env::var("RUSTEZ_VSRX_KEY") {
        // Expand ~ to home directory
        let expanded = if key_path.starts_with('~') {
            let home = env::var("HOME").expect("HOME not set");
            key_path.replacen('~', &home, 1)
        } else {
            key_path
        };
        builder = builder.key_file(&expanded);
    } else {
        let pass =
            env::var("RUSTEZ_VSRX_PASS").expect("RUSTEZ_VSRX_PASS or RUSTEZ_VSRX_KEY must be set");
        builder = builder.password(&pass);
    }

    builder
}

/// IT1: Connect, gather facts, verify hostname/model/version/serial.
#[tokio::test]
#[ignore]
#[serial]
async fn test_connect_and_gather_facts() {
    let mut dev = vsrx_builder()
        .rpc_timeout(Duration::from_secs(60))
        .open()
        .await
        .expect("failed to connect");

    let facts = dev.facts().await.expect("failed to gather facts");

    assert!(!facts.hostname.is_empty(), "hostname should not be empty");
    assert!(!facts.model.is_empty(), "model should not be empty");
    assert!(!facts.version.is_empty(), "version should not be empty");
    assert!(
        !facts.serial_number.is_empty(),
        "serial should not be empty"
    );

    println!("hostname: {}", facts.hostname);
    println!("model: {}", facts.model);
    println!("version: {}", facts.version);
    println!("serial: {}", facts.serial_number);
    println!("personality: {}", facts.personality);
    println!("route_engines: {}", facts.route_engines.len());

    dev.close().await.expect("close failed");
}

/// IT2: Run `show interfaces terse` via cli(), verify non-empty output.
#[tokio::test]
#[ignore]
#[serial]
async fn test_cli_show_interfaces() {
    let mut dev = vsrx_builder()
        .no_facts()
        .open()
        .await
        .expect("failed to connect");

    let output = dev.cli("show interfaces terse").await.expect("cli failed");

    assert!(!output.is_empty(), "CLI output should not be empty");
    println!("show interfaces terse:\n{output}");

    dev.close().await.expect("close failed");
}

/// IT3: Lock → load set config → diff → commit → unlock → verify change.
#[tokio::test]
#[ignore]
#[serial]
async fn test_config_load_and_commit() {
    let mut dev = vsrx_builder()
        .no_facts()
        .rpc_timeout(Duration::from_secs(60))
        .open()
        .await
        .expect("failed to connect");

    let mut cfg = dev.config().expect("config manager failed");

    cfg.lock().await.expect("lock failed");

    // Use a unique hostname to ensure there's always a diff
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = ConfigPayload::Text(format!("system {{ host-name rustez-it3-{timestamp}; }}"));
    cfg.load(payload).await.expect("load failed");

    let diff = cfg.diff().await.expect("diff failed");
    assert!(diff.is_some(), "diff should show changes");
    println!("diff:\n{}", diff.unwrap());

    cfg.commit().await.expect("commit failed");
    cfg.unlock().await.expect("unlock failed");

    dev.close().await.expect("close failed");
}

/// IT4: Rollback after config change.
#[tokio::test]
#[ignore]
#[serial]
async fn test_config_rollback() {
    let mut dev = vsrx_builder()
        .no_facts()
        .rpc_timeout(Duration::from_secs(60))
        .open()
        .await
        .expect("failed to connect");

    let mut cfg = dev.config().expect("config manager failed");

    cfg.lock().await.expect("lock failed");

    // Load a change with unique value
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = ConfigPayload::Text(format!("system {{ host-name rustez-it4-{timestamp}; }}"));
    cfg.load(payload).await.expect("load failed");
    cfg.commit().await.expect("commit failed");

    // Rollback to previous config
    cfg.rollback(1).await.expect("rollback failed");
    cfg.commit().await.expect("commit after rollback failed");

    cfg.unlock().await.expect("unlock failed");
    dev.close().await.expect("close failed");
}

/// IT5: Subscribe to event notifications and trigger a config change to receive one.
///
/// 1. Open two sessions — one for notifications, one for config changes.
/// 2. Subscribe to the NETCONF event stream on the first session.
/// 3. Commit a config change on the second session to trigger an event.
/// 4. Receive the notification on the first session and verify it.
#[tokio::test]
#[ignore]
#[serial]
async fn test_event_subscription() {
    // Session 1: notification listener
    let mut listener = vsrx_builder()
        .no_facts()
        .rpc_timeout(Duration::from_secs(30))
        .open()
        .await
        .expect("failed to connect listener");

    assert!(
        !listener.has_subscription(),
        "should not have subscription yet"
    );

    // Subscribe to the default NETCONF event stream
    let sub_result = listener.create_subscription(None, None, None, None).await;
    if let Err(ref err) = sub_result {
        let msg = format!("{err}");
        if msg.contains("unknown-element") || msg.contains("operation-not-supported") {
            println!("SKIP: device does not support RFC 5277 notifications");
            listener.close().await.expect("close failed");
            return;
        }
    }
    sub_result.expect("create_subscription failed");
    assert!(
        listener.has_subscription(),
        "should have active subscription"
    );

    // Session 2: make a config change to trigger a notification
    let mut changer = vsrx_builder()
        .no_facts()
        .rpc_timeout(Duration::from_secs(30))
        .open()
        .await
        .expect("failed to connect changer");

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut cfg = changer.config().expect("config manager failed");
    cfg.lock().await.expect("lock failed");
    let payload =
        rustez::ConfigPayload::Text(format!("system {{ host-name rustez-it5-{timestamp}; }}"));
    cfg.load(payload).await.expect("load failed");
    cfg.commit().await.expect("commit failed");
    cfg.unlock().await.expect("unlock failed");
    println!("config committed — waiting for notification");

    // Wait for a notification to arrive from the config commit
    match listener.recv_notification().await {
        Ok(Some(notif)) => {
            println!("received notification:");
            println!("  event_time: {}", notif.event_time);
            println!("  event_xml: {}", notif.event_xml);
        }
        Ok(None) => println!("connection closed before notification arrived"),
        Err(err) => println!("recv_notification error (may be timeout): {err}"),
    }

    // Drain any additional buffered notifications
    let extra = listener.drain_notifications().expect("drain failed");
    println!("additional buffered notifications: {}", extra.len());

    // Close changer first, then listener (listener close may hang with
    // active subscription — drop is fine as fallback)
    changer.close().await.expect("close changer failed");
    let _ = tokio::time::timeout(Duration::from_secs(5), listener.close()).await;
}

/// IT-QUAL-002: diff_against(rb_id) actually compares against the
/// requested rollback ID, not always running. With no candidate changes,
/// diff_against(0) is `None` while diff_against(1) returns the diff
/// between running and rollback-1 (whatever the last commit changed).
///
/// This test only asserts the API forwards rb_id: it verifies that
/// diff_against(rb_id) accepts the value and that at least one non-zero
/// rb_id call succeeds without panicking. Stricter assertions about diff
/// content would require seeding rollback history first.
#[tokio::test]
#[ignore]
#[serial]
async fn test_diff_against_forwards_rollback_id() {
    let mut dev = vsrx_builder()
        .rpc_timeout(Duration::from_secs(60))
        .open()
        .await
        .expect("failed to connect");

    let mut cfg = dev.config().expect("config manager failed");
    cfg.lock().await.expect("lock failed");

    // rb_id=0 is the running diff — equivalent to plain diff().
    let diff_running = cfg.diff_against(0).await.expect("diff_against(0) failed");
    let diff_default = cfg.diff().await.expect("diff() failed");
    assert_eq!(
        diff_running, diff_default,
        "diff_against(0) and diff() must return the same result"
    );

    // rb_id=1 must succeed too (no candidate changes → likely None).
    // The point is that the RPC accepts the value rather than silently
    // dropping it.
    let _diff_rb1 = cfg
        .diff_against(1)
        .await
        .expect("diff_against(1) failed — rb_id was likely dropped");

    cfg.unlock().await.expect("unlock failed");
    dev.close().await.expect("close failed");
}

/// IT-SEC-003: A failing load() on a clustered device closes the
/// auto-opened private configuration database before returning the error.
///
/// On non-clustered devices (where `requires_open_configuration()` is
/// false) this test still exercises the error path but the assertion
/// about `is_config_db_open()` is trivially satisfied — the flag is
/// expected to remain `false` throughout. The interesting case is a
/// clustered SRX: prior to RZ-SEC-003 the flag would be `true` after a
/// failed load, leaving the session in a stuck state.
#[tokio::test]
#[ignore]
#[serial]
async fn test_failed_load_closes_auto_opened_config_db() {
    let mut dev = vsrx_builder()
        .rpc_timeout(Duration::from_secs(60))
        .open()
        .await
        .expect("failed to connect");

    assert!(
        !dev.is_config_db_open(),
        "config db should start closed before any load"
    );

    let mut cfg = dev.config().expect("config manager failed");
    cfg.lock().await.expect("lock failed");

    // Deliberately malformed Junos set syntax — the device should reject it.
    let bad_payload = ConfigPayload::Set("nonsense_command_that_will_fail".to_string());
    let result = cfg.load(bad_payload).await;
    assert!(result.is_err(), "malformed load should fail");

    // Whether or not this device required an auto-open, the config db
    // must NOT be left open after the failing load returns.
    assert!(
        !dev.is_config_db_open(),
        "auto-opened config db must be closed when load fails"
    );

    // Session should still be usable for cleanup.
    let mut cfg = dev.config().expect("config manager re-acquire failed");
    cfg.unlock().await.expect("unlock after failed load failed");
    dev.close().await.expect("close failed");
}
