use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use crate::chrome_mcp_handler::chrome_instance::ChromeManager;
use crate::chrome_mcp_handler::chrome_instance::cdp_browser_manager::{
    CdpBrowserManager, RealLauncher,
};
use crate::chrome_mcp_handler::chrome_instance::launch::LaunchParams;

// These tests exercise the real Chrome lifecycle against an installed Chrome
// binary (headless). They are `#[ignore]`d because they require a working
// Chrome installation and can spawn processes; run them with:
//
//     cargo test -- --ignored
//
// Status (verified on Chrome 151, `cdp-browser-lite` 0.2.3):
//
// * B2 (attach to a user-started Chrome) PASSES — upstream 0.2.2 fixed
//   `probe::is_chrome_cdp` to send the readiness probe over HTTP/1.1
//   (Chrome >= 151 ignores HTTP/1.0 requests entirely).
//
// * B3 (a second managed instance launching on a different port/profile) PASSES
//   — upstream 0.2.3 fixed `ProfileMode::managed_lock_exists` to detect the
//   `SingletonLock` with `symlink_metadata` instead of `.exists()`. On
//   Chrome >= 151 the `SingletonLock` is a dangling symlink (its target
//   `<hostname>-<pid>` is never created; the real singleton is the socket under
//   `/tmp/com.google.Chrome.*`), so `.exists()` returned false and the library
//   fell through to `AttachAt`. With `symlink_metadata` the lock is detected and
//   a second managed instance launches on a new port/profile.
//
// The helpers below use `symlink_metadata` (not `.exists()`) for the same reason.

const TEST_PORT_BASE: u16 = 19222;
const TIMEOUT_30S: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn local_params(port: u16) -> LaunchParams {
    LaunchParams::new("127.0.0.1".into(), port, false, true, false)
}

async fn wait_for_port_close(host: &str, port: u16, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let probe = async {
            let Ok(addr) = format!("{host}:{port}").parse::<std::net::SocketAddr>() else {
                return false;
            };
            tokio::net::TcpStream::connect(&addr).await.is_ok()
        };
        if !probe.await {
            return true;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    false
}

/// HTTP/1.1 readiness probe — works on Chrome >= 151 (see NOTE above).
async fn wait_for_cdp_ready(host: &str, port: u16, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let probe = async {
            let Ok(addr) = format!("{host}:{port}").parse::<std::net::SocketAddr>() else {
                return false;
            };
            let mut stream = match tokio::net::TcpStream::connect(&addr).await {
                Ok(s) => s,
                Err(_) => return false,
            };
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let req = format!(
                "GET /json/version HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(req.as_bytes()).await.is_err() {
                return false;
            }
            let mut response = String::new();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        response.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if response.contains("Browser") || response.contains("WebKit-Version") {
                            return true;
                        }
                        if response.len() > 64 * 1024 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            response.contains("Browser") || response.contains("WebKit-Version")
        };
        if probe.await {
            return true;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    false
}

fn singleton_lock_path(port: u16) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("chrome-mcp-profile-{port}"))
}

async fn wait_for_lock(port: u16, timeout: Duration) -> bool {
    let lock = singleton_lock_path(port).join("SingletonLock");
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        // Chrome >= 151 leaves `SingletonLock` as a dangling symlink, so use
        // symlink_metadata (which does not follow the target), matching the
        // library's fixed `managed_lock_exists`.
        if std::fs::symlink_metadata(&lock).is_ok() {
            return true;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    false
}

#[tokio::test]
#[ignore = "requires a real Chrome installation"]
async fn given_real_chrome_when_ensure_and_client_then_browser_version_responds() {
    let port = TEST_PORT_BASE;
    let mut mgr = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    mgr.ensure_instance()
        .await
        .expect("ensure_instance must succeed against real Chrome");
    assert!(
        wait_for_cdp_ready("127.0.0.1", mgr.get_port(), TIMEOUT_30S).await,
        "managed Chrome must be reachable over HTTP/1.1 CDP"
    );
    let client = mgr.client().await.expect("client() must succeed");
    let _ = client
        .send_raw_command("Runtime.enable", cdp_browser_lite::NoParams)
        .await;
}

#[tokio::test]
#[ignore = "requires a real Chrome installation"]
async fn given_managed_instance_when_stop_instance_then_process_is_gone() {
    let port = TEST_PORT_BASE + 1;
    let mut mgr = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    mgr.ensure_instance()
        .await
        .expect("ensure_instance must succeed");
    assert!(
        wait_for_cdp_ready("127.0.0.1", mgr.get_port(), TIMEOUT_30S).await,
        "managed Chrome must be reachable before stop"
    );
    let active_port = mgr.get_port();
    mgr.stop_instance()
        .await
        .expect("stop_instance must succeed");
    assert!(
        wait_for_port_close("127.0.0.1", active_port, TIMEOUT_30S).await,
        "managed Chrome process must be terminated within {TIMEOUT_30S:?}"
    );
}

#[tokio::test]
#[ignore = "requires a real Chrome installation; covers D1 / B16"]
async fn given_managed_instance_when_manager_dropped_then_process_is_gone() {
    let port = TEST_PORT_BASE + 2;
    let mut mgr = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    mgr.ensure_instance()
        .await
        .expect("ensure_instance must succeed");
    assert!(
        wait_for_cdp_ready("127.0.0.1", mgr.get_port(), TIMEOUT_30S).await,
        "managed Chrome must be reachable before drop"
    );
    let active_port = mgr.get_port();
    drop(mgr);
    assert!(
        wait_for_port_close("127.0.0.1", active_port, TIMEOUT_30S).await,
        "dropping the manager must terminate the managed Chrome within {TIMEOUT_30S:?} (D1)"
    );
}

#[tokio::test]
#[ignore = "requires a real Chrome installation; covers B2"]
async fn given_attached_instance_when_stop_instance_then_process_survives() {
    let port = TEST_PORT_BASE + 3;
    let mut user_chrome = spawn_user_chrome_on(port);
    assert!(
        wait_for_cdp_ready("127.0.0.1", port, TIMEOUT_30S).await,
        "user-spawned Chrome must be CDP-ready on port {port}"
    );
    let mut mgr = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    mgr.ensure_instance()
        .await
        .expect("ensure_instance must attach to user Chrome");
    assert_eq!(
        mgr.get_port(),
        port,
        "manager must report the user Chrome's port (B2)"
    );
    mgr.stop_instance()
        .await
        .expect("stop_instance must succeed");
    assert!(
        wait_for_cdp_ready("127.0.0.1", port, Duration::from_secs(1)).await,
        "attached Chrome must still be CDP-ready after stop (B2)"
    );
    let _ = user_chrome.kill().await;
}

fn spawn_user_chrome_on(port: u16) -> tokio::process::Child {
    let path = cdp_browser_lite::discovery::discover_default()
        .unwrap_or_else(|_| std::path::PathBuf::from("google-chrome"));
    let _ = std::fs::remove_dir_all(format!("/tmp/chrome-debug-mcp-attached-test-{port}"));
    tokio::process::Command::new(&path)
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-session-crashed-bubble")
        .arg("--noerrdialogs")
        .arg("--disable-dev-shm-usage")
        .arg("--headless=new")
        .arg(format!(
            "--user-data-dir=/tmp/chrome-debug-mcp-attached-test-{port}"
        ))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawning user-style Chrome must succeed")
}

#[tokio::test]
#[ignore = "requires a real Chrome installation; covers B3"]
async fn given_managed_instance_on_configured_port_when_second_manager_ensures_then_uses_different_port_and_profile()
 {
    let port = TEST_PORT_BASE + 4;
    let mut first = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    first
        .ensure_instance()
        .await
        .expect("first ensure must succeed");
    let first_port = first.get_port();
    assert!(
        wait_for_lock(first_port, TIMEOUT_30S).await,
        "managed instance must leave a SingletonLock in its profile"
    );

    let mut second = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    second
        .ensure_instance()
        .await
        .expect("second ensure must succeed");
    let second_port = second.get_port();
    assert_ne!(
        second_port, first_port,
        "library must reassign to a different port (B3)"
    );
    assert!(
        wait_for_lock(second_port, TIMEOUT_30S).await,
        "second managed instance must create its own profile SingletonLock"
    );
    assert!(
        std::fs::symlink_metadata(singleton_lock_path(first_port).join("SingletonLock")).is_ok(),
        "first managed instance's SingletonLock must remain untouched (B3)"
    );

    let _ = first.stop_instance().await;
    let _ = second.stop_instance().await;
}

#[tokio::test]
#[ignore = "requires a real Chrome installation; covers B3 lock survival"]
async fn given_managed_instance_on_configured_port_when_second_manager_ensures_then_existing_singleton_lock_survives()
 {
    let port = TEST_PORT_BASE + 5;
    let mut first = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    first
        .ensure_instance()
        .await
        .expect("first ensure must succeed");
    let first_port = first.get_port();
    assert!(
        wait_for_lock(first_port, TIMEOUT_30S).await,
        "managed instance must leave a SingletonLock in its profile"
    );
    let lock_before = singleton_lock_path(first_port).join("SingletonLock");
    // Chrome >= 151 leaves SingletonLock as a dangling symlink, so use
    // symlink_metadata (which does not follow the target) to read its own inode.
    let inode_before = std::fs::symlink_metadata(&lock_before)
        .expect("lock must be readable")
        .ino();

    let mut second = CdpBrowserManager::new(local_params(port), Box::new(RealLauncher));
    let _ = second.ensure_instance().await;
    assert!(
        std::fs::symlink_metadata(&lock_before).is_ok(),
        "pre-existing SingletonLock must survive the second manager's ensure"
    );
    let inode_after = std::fs::symlink_metadata(&lock_before)
        .expect("lock must still be readable")
        .ino();
    assert_eq!(
        inode_before, inode_after,
        "lock inode must be unchanged — the second manager must not delete the first instance's lock"
    );

    let _ = first.stop_instance().await;
    let _ = second.stop_instance().await;
}
