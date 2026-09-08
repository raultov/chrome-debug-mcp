use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Os {
    Linux,
    Mac,
    Windows,
}

pub(crate) trait EnvProvider {
    fn var(&self, name: &str) -> Option<String>;
    fn current_os(&self) -> Os;
}

pub(crate) trait FsProbe {
    fn exists(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> Result<String, std::io::Error>;
}

pub(crate) struct RealEnvProvider;

impl EnvProvider for RealEnvProvider {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn current_os(&self) -> Os {
        if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

pub(crate) struct RealFsProbe;

impl FsProbe for RealFsProbe {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_to_string(&self, path: &Path) -> Result<String, std::io::Error> {
        fs::read_to_string(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CookieSource {
    pub(crate) root: PathBuf,
    pub(crate) profile: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SeedReport {
    pub(crate) dir: PathBuf,
}

pub(crate) fn source_roots(os: Os, env: &dyn EnvProvider) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    match os {
        Os::Linux => {
            if let Some(config_home) = env.var("XDG_CONFIG_HOME") {
                roots.push(PathBuf::from(config_home).join("google-chrome"));
            }
            if let Some(home) = env.var("HOME") {
                roots.push(PathBuf::from(&home).join(".config/google-chrome"));
                roots.push(PathBuf::from(&home).join(".config/chromium"));
                roots.push(PathBuf::from(&home).join("snap/chromium/common/chromium"));
            }
        }
        Os::Mac => {
            if let Some(home) = env.var("HOME") {
                roots.push(PathBuf::from(&home).join("Library/Application Support/Google/Chrome"));
                roots.push(PathBuf::from(&home).join("Library/Application Support/Chromium"));
            }
        }
        Os::Windows => {
            if let Some(local_app_data) = env.var("LOCALAPPDATA") {
                roots.push(
                    PathBuf::from(&local_app_data)
                        .join("Google")
                        .join("Chrome")
                        .join("User Data"),
                );
                roots.push(
                    PathBuf::from(&local_app_data)
                        .join("Chromium")
                        .join("User Data"),
                );
            }
        }
    }

    roots
}

pub(crate) fn pick_profile(
    root: &Path,
    override_profile: Option<&str>,
    fs: &dyn FsProbe,
) -> String {
    if let Some(p) = override_profile {
        return p.to_string();
    }

    let local_state_path = root.join("Local State");
    if let Ok(content) = fs.read_to_string(&local_state_path)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(last_used) = json
            .get("profile")
            .and_then(|p| p.get("last_used"))
            .and_then(|u| u.as_str())
    {
        return last_used.to_string();
    }

    "Default".to_string()
}

pub(crate) fn locate_cookies_db(root: &Path, profile: &str, fs: &dyn FsProbe) -> Option<PathBuf> {
    let prof_dir = root.join(profile);

    // Check <profile>/Network/Cookies first (newer Chrome versions)
    let net_cookies = prof_dir.join("Network").join("Cookies");
    if fs.is_file(&net_cookies) {
        return Some(net_cookies);
    }

    // Check <profile>/Cookies (older Chrome versions or specific installs)
    let root_cookies = prof_dir.join("Cookies");
    if fs.is_file(&root_cookies) {
        return Some(root_cookies);
    }

    None
}

/// Merges `os_crypt` from `src_local_state_str` into `dst_local_state_str` (or creates a minimal JSON).
pub(crate) fn merge_os_crypt(
    src_local_state_str: &str,
    dst_local_state_str: Option<&str>,
) -> serde_json::Value {
    let src_json: serde_json::Value =
        serde_json::from_str(src_local_state_str).unwrap_or(serde_json::Value::Null);

    let mut dst_json: serde_json::Value = dst_local_state_str
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if let Some(os_crypt) = src_json.get("os_crypt")
        && let Some(dst_obj) = dst_json.as_object_mut()
    {
        dst_obj.insert("os_crypt".to_string(), os_crypt.clone());
    }

    dst_json
}

/// Environment variables the seeded Chrome needs to reach the OS secret
/// backend (portal / keyring) so the imported `v11` cookies can be decrypted.
///
/// Chrome derives the cookie-encryption key from the desktop secret store.
/// When the MCP server is spawned with a sanitized environment (e.g. by an
/// editor/agent host that drops `DBUS_SESSION_BUS_ADDRESS`), Chrome cannot
/// reach that store, silently falls back to the `basic` password, and every
/// imported cookie is discarded. This returns the missing variables derived
/// from the real user session; variables already present in the process
/// environment are omitted (children inherit them anyway).
///
/// On non-Linux platforms the secret store needs no D-Bus session, so this
/// returns an empty list.
pub(crate) fn desktop_session_env() -> Vec<(String, String)> {
    let lookup = |name: &str| std::env::var(name).ok();
    let uid = current_uid();
    desktop_session_env_with(&lookup, uid)
}

/// Pure core of [`desktop_session_env`], injectable for tests.
pub(crate) fn desktop_session_env_with(
    lookup: &dyn Fn(&str) -> Option<String>,
    uid: Option<u32>,
) -> Vec<(String, String)> {
    if cfg!(not(target_os = "linux")) {
        return Vec::new();
    }

    let runtime_dir = match lookup("XDG_RUNTIME_DIR") {
        Some(dir) => Some(dir),
        None => uid.map(|id| format!("/run/user/{id}")),
    };

    let mut vars = Vec::new();
    if lookup("XDG_RUNTIME_DIR").is_none()
        && let Some(ref dir) = runtime_dir
    {
        vars.push(("XDG_RUNTIME_DIR".to_string(), dir.clone()));
    }
    if lookup("DBUS_SESSION_BUS_ADDRESS").is_none()
        && let Some(ref dir) = runtime_dir
    {
        vars.push((
            "DBUS_SESSION_BUS_ADDRESS".to_string(),
            format!("unix:path={dir}/bus"),
        ));
    }
    vars
}

/// Reads the real UID of the current process from `/proc/self/status`
/// (Linux only). Returns `None` when unavailable.
pub(crate) fn current_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

const SEED_PREFIX: &str = "chrome-debug-mcp-profile-";

/// Creates a temporary profile directory with restricted permissions (`0700` on Unix).
pub(crate) fn create_seed_dir() -> Result<PathBuf, String> {
    sweep_orphaned_seed_dirs();

    let temp = tempfile::Builder::new()
        .prefix(SEED_PREFIX)
        .tempdir()
        .map_err(|e| format!("Failed to create seed directory: {e}"))?;

    let path = temp.keep();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
    }

    Ok(path)
}

fn sweep_orphaned_seed_dirs() {
    let root = std::env::temp_dir();
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let max_age = std::time::Duration::from_secs(24 * 3600);

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(SEED_PREFIX) {
            continue;
        }
        let old_enough = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|m| {
                now.duration_since(m)
                    .map(|age| age >= max_age)
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if old_enough {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

pub(crate) fn resolve_source(
    requested_profile: Option<&str>,
    env: &dyn EnvProvider,
    fs: &dyn FsProbe,
) -> Result<CookieSource, String> {
    let os = env.current_os();
    let roots = source_roots(os, env);

    let mut searched_paths = Vec::new();

    for root in &roots {
        if !fs.exists(root) {
            searched_paths.push(format!("Root not found: {}", root.display()));
            continue;
        }

        let profile = pick_profile(root, requested_profile, fs);
        if let Some(_cookies_db) = locate_cookies_db(root, &profile, fs) {
            return Ok(CookieSource {
                root: root.clone(),
                profile,
            });
        } else {
            searched_paths.push(format!(
                "Cookies DB not found in root: {} (profile: {})",
                root.display(),
                profile
            ));
        }
    }

    Err(format!(
        "No Chrome profile with cookies found. Searched locations:\n  - {}",
        searched_paths.join("\n  - ")
    ))
}

pub(crate) fn seed_cookies(
    source: &CookieSource,
    env: &dyn EnvProvider,
    fs_probe: &dyn FsProbe,
) -> Result<SeedReport, String> {
    let cookies_db =
        locate_cookies_db(&source.root, &source.profile, fs_probe).ok_or_else(|| {
            format!(
                "Cookies DB not found for profile '{}' at '{}'",
                source.profile,
                source.root.display()
            )
        })?;

    let seed_dir = create_seed_dir()?;
    let dst_default = seed_dir.join("Default");

    fs::create_dir_all(&dst_default)
        .map_err(|e| format!("Failed to create seed subdirectory: {e}"))?;

    let mut copied_files = Vec::new();

    // Chrome reads the cookie DB from `<profile>/Cookies` (older layouts) or
    // `<profile>/Network/Cookies` (current layouts). The spawned browser may
    // use either layout regardless of where the source profile keeps it, so
    // the DB is copied to both candidate paths; Chrome reads the one matching
    // its layout and ignores the other.
    let relative = cookies_db
        .strip_prefix(source.root.join(&source.profile))
        .map(|p| p.to_path_buf())
        .ok();

    let mut dst_candidates: Vec<PathBuf> = Vec::new();
    if let Some(rel) = &relative {
        dst_candidates.push(dst_default.join(rel));
    }
    dst_candidates.push(dst_default.join("Cookies"));
    dst_candidates.push(dst_default.join("Network").join("Cookies"));
    dst_candidates.dedup();

    for dst in &dst_candidates {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create seed subdirectory {}: {e}",
                    parent.display()
                )
            })?;
        }
        fs::copy(&cookies_db, dst)
            .map_err(|e| format!("Failed to copy Cookies DB to '{}': {e}", dst.display()))?;
        copied_files.push(format!(
            "Default/{}",
            dst.strip_prefix(&dst_default)
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ));
    }

    // Copy the sidecar files (journal/WAL) for every copied DB path so that
    // unflushed transactions are not lost regardless of the reader's layout.
    for dst in &dst_candidates {
        for suffix in ["-journal", "-wal", "-shm"] {
            let sidecar = cookies_db.with_file_name(format!("Cookies{suffix}"));
            if fs_probe.is_file(&sidecar) {
                let dst_sidecar = dst.with_file_name(format!("Cookies{suffix}"));
                if fs::copy(&sidecar, &dst_sidecar).is_ok() {
                    copied_files.push(format!(
                        "Default/{}",
                        dst_sidecar
                            .strip_prefix(&dst_default)
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    ));
                }
            }
        }
    }

    // Merge os_crypt into Local State
    let src_local_state_path = source.root.join("Local State");
    if fs_probe.is_file(&src_local_state_path)
        && let Ok(src_content) = fs_probe.read_to_string(&src_local_state_path)
    {
        let merged = merge_os_crypt(&src_content, None);
        let dst_local_state_path = seed_dir.join("Local State");
        if let Ok(merged_str) = serde_json::to_string_pretty(&merged)
            && fs::write(&dst_local_state_path, merged_str).is_ok()
        {
            copied_files.push("Local State".to_string());
        }
    }

    let _ = env; // for future extensions if needed

    Ok(SeedReport { dir: seed_dir })
}

/// Validates the cookie-import prerequisites and, when requested, seeds a
/// fresh profile directory from the user's real Chrome profile.
///
/// Returns `Ok(None)` when the import was not requested; an error string
/// when the request violates the server configuration; otherwise the
/// seed report to hand to the browser manager.
pub(crate) fn prepare_seed_report(
    requested: bool,
    source_profile: Option<&str>,
    allow_cookie_import: bool,
    user_profile: bool,
) -> Result<Option<SeedReport>, String> {
    if !requested {
        return Ok(None);
    }
    if !allow_cookie_import {
        return Err("Cookie import is disabled. Restart the MCP server with the '--allow-cookie-import' argument to enable it.".to_string());
    }
    if user_profile {
        return Err("Cookie import is redundant when running in --user-profile mode, as the user's real profile is already in use.".to_string());
    }

    let source = resolve_source(source_profile, &RealEnvProvider, &RealFsProbe)?;
    let report = seed_cookies(&source, &RealEnvProvider, &RealFsProbe)?;
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct MockEnv {
        os: Os,
        vars: HashMap<String, String>,
    }

    impl MockEnv {
        fn new(os: Os, vars: &[(&str, &str)]) -> Self {
            Self {
                os,
                vars: vars
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            }
        }
    }

    impl EnvProvider for MockEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }

        fn current_os(&self) -> Os {
            self.os
        }
    }

    struct MockFs {
        files: HashSet<PathBuf>,
        content: HashMap<PathBuf, String>,
    }

    impl MockFs {
        fn new(files: &[&str], content: &[(&str, &str)]) -> Self {
            Self {
                files: files.iter().map(PathBuf::from).collect(),
                content: content
                    .iter()
                    .map(|(k, v)| (PathBuf::from(k), v.to_string()))
                    .collect(),
            }
        }
    }

    impl FsProbe for MockFs {
        fn exists(&self, path: &Path) -> bool {
            self.files.contains(path) || self.content.contains_key(path)
        }

        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path) || self.content.contains_key(path)
        }

        fn read_to_string(&self, path: &Path) -> Result<String, std::io::Error> {
            self.content
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        }
    }

    #[test]
    fn given_linux_os_when_getting_source_roots_then_includes_xdg_and_home() {
        let env = MockEnv::new(
            Os::Linux,
            &[
                ("XDG_CONFIG_HOME", "/custom/config"),
                ("HOME", "/home/testuser"),
            ],
        );
        let roots = source_roots(Os::Linux, &env);
        assert_eq!(roots[0], PathBuf::from("/custom/config/google-chrome"));
        assert_eq!(
            roots[1],
            PathBuf::from("/home/testuser/.config/google-chrome")
        );
    }

    #[test]
    fn given_mac_os_when_getting_source_roots_then_includes_application_support() {
        let env = MockEnv::new(Os::Mac, &[("HOME", "/Users/testuser")]);
        let roots = source_roots(Os::Mac, &env);
        assert_eq!(
            roots[0],
            PathBuf::from("/Users/testuser/Library/Application Support/Google/Chrome")
        );
    }

    #[test]
    fn given_windows_os_when_getting_source_roots_then_includes_localappdata() {
        let env = MockEnv::new(
            Os::Windows,
            &[("LOCALAPPDATA", r"C:\Users\test\AppData\Local")],
        );
        let roots = source_roots(Os::Windows, &env);
        let expected = PathBuf::from(r"C:\Users\test\AppData\Local")
            .join("Google")
            .join("Chrome")
            .join("User Data");
        assert_eq!(roots[0], expected);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn given_dbus_present_when_computing_desktop_env_then_nothing_is_injected() {
        let lookup = |name: &str| {
            if name == "DBUS_SESSION_BUS_ADDRESS" {
                Some("unix:path=/run/user/1000/bus".to_string())
            } else if name == "XDG_RUNTIME_DIR" {
                Some("/run/user/1000".to_string())
            } else {
                None
            }
        };
        let vars = desktop_session_env_with(&lookup, Some(1000));
        assert!(vars.is_empty(), "got {vars:?}");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn given_runtime_dir_present_when_computing_desktop_env_then_only_dbus_is_derived() {
        let lookup = |name: &str| (name == "XDG_RUNTIME_DIR").then(|| "/run/user/1000".to_string());
        let vars = desktop_session_env_with(&lookup, None);
        assert_eq!(
            vars,
            vec![(
                "DBUS_SESSION_BUS_ADDRESS".to_string(),
                "unix:path=/run/user/1000/bus".to_string()
            )]
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn given_nothing_present_when_computing_desktop_env_then_both_are_derived_from_uid() {
        let lookup = |_name: &str| None::<String>;
        let vars = desktop_session_env_with(&lookup, Some(1000));
        assert_eq!(
            vars,
            vec![
                ("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string()),
                (
                    "DBUS_SESSION_BUS_ADDRESS".to_string(),
                    "unix:path=/run/user/1000/bus".to_string()
                ),
            ]
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn given_no_uid_when_computing_desktop_env_then_nothing_is_injected() {
        let lookup = |_name: &str| None::<String>;
        let vars = desktop_session_env_with(&lookup, None);
        assert!(vars.is_empty(), "got {vars:?}");
    }

    #[test]
    fn given_proc_status_when_reading_uid_then_it_is_parsed() {
        let uid = current_uid();
        assert!(uid.is_some(), "uid should be readable on this Linux host");
    }

    #[test]
    fn given_local_state_with_last_used_when_picking_profile_then_returns_last_used() {
        let root = Path::new("/root");
        let fs = MockFs::new(
            &[],
            &[(
                "/root/Local State",
                r#"{"profile":{"last_used":"Profile 2"}}"#,
            )],
        );
        assert_eq!(pick_profile(root, None, &fs), "Profile 2");
    }

    #[test]
    fn given_override_profile_when_picking_profile_then_returns_override() {
        let root = Path::new("/root");
        let fs = MockFs::new(&[], &[]);
        assert_eq!(pick_profile(root, Some("Profile 1"), &fs), "Profile 1");
    }

    #[test]
    fn given_no_last_used_when_picking_profile_then_defaults_to_default() {
        let root = Path::new("/root");
        let fs = MockFs::new(&[], &[]);
        assert_eq!(pick_profile(root, None, &fs), "Default");
    }

    #[test]
    fn given_net_cookies_when_locating_cookies_db_then_finds_net_cookies() {
        let root = Path::new("/root");
        let fs = MockFs::new(&["/root/Default/Network/Cookies"], &[]);
        assert_eq!(
            locate_cookies_db(root, "Default", &fs),
            Some(PathBuf::from("/root/Default/Network/Cookies"))
        );
    }

    #[test]
    fn given_root_cookies_when_locating_cookies_db_then_finds_root_cookies() {
        let root = Path::new("/root");
        let fs = MockFs::new(&["/root/Default/Cookies"], &[]);
        assert_eq!(
            locate_cookies_db(root, "Default", &fs),
            Some(PathBuf::from("/root/Default/Cookies"))
        );
    }

    #[test]
    fn given_src_os_crypt_when_merging_os_crypt_then_inserts_into_dst() {
        let src = r#"{"os_crypt":{"encrypted_key":"ABC","selected_backend":"portal"}}"#;
        let merged = merge_os_crypt(src, None);
        assert_eq!(merged["os_crypt"]["encrypted_key"], "ABC");
        assert_eq!(merged["os_crypt"]["selected_backend"], "portal");
    }

    #[test]
    fn given_valid_source_when_resolving_source_then_returns_cookie_source() {
        let env = MockEnv::new(Os::Linux, &[("HOME", "/home/test")]);
        let fs = MockFs::new(
            &[
                "/home/test/.config/google-chrome",
                "/home/test/.config/google-chrome/Default/Network/Cookies",
            ],
            &[(
                "/home/test/.config/google-chrome/Local State",
                r#"{"profile":{"last_used":"Default"}}"#,
            )],
        );

        let source = resolve_source(None, &env, &fs).unwrap();
        assert_eq!(
            source.root,
            PathBuf::from("/home/test/.config/google-chrome")
        );
        assert_eq!(source.profile, "Default");
    }

    #[test]
    fn given_no_valid_source_when_resolving_source_then_returns_error_with_searched_paths() {
        let env = MockEnv::new(Os::Linux, &[("HOME", "/home/test")]);
        let fs = MockFs::new(&[], &[]);

        let err = resolve_source(None, &env, &fs).unwrap_err();
        assert!(err.contains("No Chrome profile with cookies found"));
        assert!(err.contains("/home/test/.config/google-chrome"));
    }

    #[test]
    fn given_not_requested_when_preparing_seed_report_then_returns_none() {
        let report = prepare_seed_report(false, None, true, false).unwrap();
        assert!(report.is_none());
    }

    #[test]
    fn given_import_disabled_when_preparing_seed_report_then_rejects() {
        let err = prepare_seed_report(true, None, false, false).unwrap_err();
        assert!(err.contains("Cookie import is disabled"), "{err}");
    }

    #[test]
    fn given_user_profile_mode_when_preparing_seed_report_then_rejects() {
        let err = prepare_seed_report(true, None, true, true).unwrap_err();
        assert!(err.contains("--user-profile mode"), "{err}");
    }
}
