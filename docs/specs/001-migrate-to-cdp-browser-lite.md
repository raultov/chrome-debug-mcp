# Spec 001 — Migrate Chrome lifecycle management to `cdp-browser-lite`

- **Status:** Implemented (targets `cdp-browser-lite` 0.2.1)
- **Date:** 2026-08-19 (revised for `cdp-browser-lite` 0.2.1)
- **Target version:** 1.1.0
- **Affected crate:** `chrome-debug-mcp`
- **New dependency:** `cdp-browser-lite = "0.2.4"` (replaces the direct `cdp-lite = "0.1.3"` dependency)

---

## 1. Goal

Replace the hand-rolled Chrome lifecycle manager (`ChromeInstanceManager`,
`src/chrome_mcp_handler/chrome_instance/mod.rs`, ~420 lines of implementation plus
~450 lines of tests) with `cdp-browser-lite`, while preserving the current
observable behaviour of the MCP server.

`cdp-browser-lite` 0.2.0 still depends on `cdp-lite` 0.1.3 — the exact version already
in use — and re-exports its whole surface, so `CdpClient`, `CdpError`, `NoParams`,
`WsResponse` and `EventFilter` are unchanged. **No CDP-protocol-level behaviour
changes.** This migration is confined to process lifecycle: discovery, spawn, port
selection, profile handling and termination.

**All lifecycle policy is delegated.** The MCP keeps no port probing, no free-port
scan and no lock-file inspection of its own; `LaunchMode::Auto` plus
`ProfileMode::PersistentPerPort` cover the entire decision tree.

### Non-goals

- Changing any of the 24 MCP tool schemas or their behaviour.
- Changing the CDP client, its event broadcast channel or the listener wiring.
- Adding new CLI flags.
- Managing more than one Chrome instance. `cdp-browser-lite` 0.2.0 ships `BrowserPool`
  for that; the MCP server is single-instance by design. If multi-session support is
  ever wanted, the pool is the vehicle — out of scope here.

---

## 2. Decisions taken

| # | Decision | Rationale |
|---|---|---|
| D1 | `keep_alive_on_drop(false)` — automatic cleanup, no zombie Chrome processes. | Dropping `Browser` terminates managed Chrome. Accepted behaviour change: today Chrome survives an MCP-server crash and is re-attached on restart; from now on it is killed. |
| D2 | Managed-instance detection **must be preserved**: a deterministic profile directory per port, so the server can tell "Chrome I launched" from "Chrome the user launched". | This is what drives the attach-vs-jump-to-another-port decision. Losing it means always attaching to whatever sits on 9222. |

### 2.1 Consequence of D1

`Browser::drop` (`browser.rs:403-419`) performs a best-effort synchronous `start_kill()`
guarded by `try_lock`. If the state mutex is contended at drop time **nothing happens and
the process leaks**. Additionally `ChromeProcess` spawns the child with
`kill_on_drop(!keep_alive_on_drop)`, so with D1 the Tokio runtime reaps it. The explicit
`stop_chrome` tool remains the reliable path; `Drop` is the safety net.

Attached (non-managed) browsers are never killed — neither on `stop()` nor on `Drop`.
This matches today's behaviour, where `stop_instance_impl` only kills `self.child`
(which is `None` when attached).

### 2.2 How D2 is satisfied

`ProfileMode::PersistentPerPort { root, prefix }` derives the profile directory from the
**resolved** port (`root/{prefix}{port}`), which is exactly the deterministic,
cross-process-observable marker D2 needs: any process can answer "is port P held by a
managed instance?" by testing `root/{prefix}{P}/SingletonLock`.

`LaunchMode::Auto` consumes it directly. `Browser::decide_auto`
(`cdp-browser-lite/src/browser.rs:114-144`) implements the decision tree the MCP needs,
one-to-one with today's `ensure_instance_impl`:

```rust
if !probe::is_port_open(host, port)          -> LaunchAt(port)
if !probe::is_chrome_cdp(host, port)         -> LaunchAt(allocator.reserve_near(..))
if config.profile.managed_lock_exists(port)  -> LaunchAt(allocator.reserve_near(
                                                    .., |p| !profile.managed_lock_exists(p)))
otherwise                                    -> AttachAt(port)
```

`spawn_managed` then calls `Profile::prepare(&config.profile, config.port)` with the
**new** port, so the directory tracks the port and `remove_singleton_lock()` only ever
touches the freshly prepared profile — never the lock of a live instance.

> Historical note: 0.1.1 reused one `Profile` across the port jump and deleted the live
> instance's `SingletonLock`, which forced an earlier revision of this spec to resolve
> the port inside the MCP. Fixed in 0.2.0; that workaround is gone.

Minor difference from today: the library searches `[port, port + 100)` whereas
`find_new_port` searched `(port, port + 100]`. Since the base port is by definition
occupied when the search runs, the bind probe rejects it and the outcome is equivalent.

---

## 3. Behaviour parity contract

These are the invariants the migration must not break. Each maps to a scenario in
Section 6.

| ID | Given | When | Then |
|---|---|---|---|
| B1 | Nothing listening on the configured port | `ensure_instance` | A managed Chrome is spawned on that port with profile `chrome-mcp-profile-{port}` |
| B2 | A user-started Chrome on the port (CDP responds, no managed lock file) | `ensure_instance` | Attach to it; do **not** spawn; do **not** kill it on `stop_chrome` |
| B3 | Another *managed* instance on the port (CDP responds, lock file present) | `ensure_instance` | Spawn on the next free port N, with profile `chrome-mcp-profile-N`, leaving the existing lock intact |
| B4 | A non-Chrome process on the port, host local | `ensure_instance` | Spawn on the next free port |
| B5 | A non-Chrome process on the port, host remote | `ensure_instance` | Error; never spawn remotely |
| B6 | Host is not local and no Chrome is reachable | `ensure_instance` | Error; never spawn remotely |
| B7 | `--user-profile` given | `ensure_instance` | No `--user-data-dir` flag emitted |
| B8 | `--headless` given | `ensure_instance` | `--headless=new` and `--no-sandbox` emitted |
| B9 | `CHROME_NO_SANDBOX` set, not headless | `ensure_instance` | `--no-sandbox` emitted |
| B10 | `enable_automation = false` | `ensure_instance` | `--disable-infobars` emitted, `--enable-automation` not emitted |
| B11 | `enable_automation = true` | `ensure_instance` | `--enable-automation` emitted |
| B12 | `CHROME_PATH` set | `ensure_instance` | That executable is used |
| B13 | A managed instance is running | `restart_chrome` with `proxy_server` | Old instance stopped, new one spawned with `--proxy-server=...` |
| B14 | Port was dynamically reassigned to N | any tool call | `get_port()` returns N, and the CDP client connects to N |
| B15 | `stop_chrome` was called | a subsequent tool call | A fresh instance is ensured |
| B16 | Chrome was started successfully | The MCP process exits | The managed Chrome process is terminated (**D1 — new behaviour**) |

B4, B5 and B6 are enforced by the library: `BrowserConfig::validate()` rejects `Auto`
and `LaunchNew` on a non-local host, so the remote cases can only ever be `AttachOnly`,
which fails with `RemoteUnavailable` instead of spawning.

`--disable-gpu` must remain absent (WebGL fix from v1.0.8). `cdp-browser-lite` does not
emit it. The six base flags (`--no-first-run`, `--no-default-browser-check`,
`--disable-session-crashed-bubble`, `--noerrdialogs`, `--disable-dev-shm-usage`, plus
`--remote-debugging-port`) are emitted by the library.

---

## 4. Target architecture

```
ChromeMcpHandler
  └── chrome_manager: Arc<Mutex<dyn ChromeManager>>
        ├── CdpBrowserManager        (production — new)
        │     ├── LaunchParams       (CLI-derived, mutable: proxy changes at runtime)
        │     ├── LaunchPlan         (mode + profile + extra args)  -> pure
        │     ├── resolved_port: u16 (cached at ensure time)
        │     └── Box<dyn ManagedBrowser>
        │           ├── RealBrowser(cdp_browser_lite::Browser)
        │           └── FakeBrowser  (tests)
        └── MockChromeManager        (tests — kept, ~20 test modules depend on it)
```

### 4.1 Trait evolution

```rust
#[async_trait]
pub trait ChromeManager: Send + Sync {
    async fn ensure_instance(&mut self) -> anyhow::Result<()>;
    async fn stop_instance(&mut self) -> anyhow::Result<()>;   // was sync
    async fn client(&self) -> anyhow::Result<CdpClient>;       // new
    fn get_port(&self) -> u16;                                 // stays sync — see below
    fn set_proxy(&mut self, proxy: Option<String>);
}
```

- `stop_instance` becomes `async` because `Browser::stop()` is async. Both call sites
  (`restart_chrome.rs:36`, `stop_chrome.rs:27`) are already in async context.
- `client()` is added so `get_or_connect` no longer builds the address itself. The
  production impl returns `browser.client()`; `MockChromeManager` returns
  `CdpClient::new(&format!("127.0.0.1:{}", self.port), Duration::from_secs(10))`, i.e.
  exactly what `get_or_connect` does today — so all ~20 existing test modules that pair
  `MockChromeManager::new(port)` with a mock WebSocket server keep working unchanged.
- `set_port` is dropped (it is already `#[allow(dead_code)]`, and the `#[allow]`
  attribute is forbidden by the project conventions).

**`get_port` deliberately stays synchronous.** In 0.2.0 `Browser::debug_address` became
`async fn debug_address(&self) -> (String, u16)`. Rather than propagate `async` into
`ChromeManager::get_port` — which would ripple into `get_or_connect`
(`chrome_mcp_handler/mod.rs:209`), `get_performance_metrics.rs:74` and every
`MockChromeManager` call site — `CdpBrowserManager` caches the resolved port in a plain
field, written once inside `ensure_instance` from `browser.debug_address().await.1`.
This mirrors how the library's own `BrowserEntry` snapshots port and profile metadata at
open time instead of re-reading them through the state mutex.

### 4.2 New types

```rust
/// CLI-derived launch inputs. Mutable: `restart_chrome` can change the proxy.
pub(crate) struct LaunchParams {
    host: String,
    port: u16,               // the configured port; the library may resolve a different one
    headless: bool,
    enable_automation: bool,
    user_profile: bool,
    proxy: Option<String>,
}

/// Fully resolved launch instruction. All three field types derive PartialEq,
/// so this is directly assertable in unit tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchPlan {
    mode: LaunchMode,
    profile: ProfileMode,
    extra_args: Vec<String>,
}
```

**Why `LaunchPlan` and not `BrowserConfig` as the unit under test:** verified against
0.2.0 — `BrowserConfig` is still `#[derive(Debug, Clone)]` with all fields `pub(crate)`,
and its only public surface is `builder()`, `port()` and `validate()`. It derives neither
`PartialEq` nor `Default` (the crate documents this as deliberate). Asserting on it from
outside would require `Debug`-string matching. `LaunchMode` and `ProfileMode` *do* derive
`PartialEq, Eq`, so `LaunchPlan` gives a clean assertion surface. `LaunchPlan ->
BrowserConfig` is then a mechanical, untested adapter.

### 4.3 The `ManagedBrowser` seam

`Browser` is a concrete struct that spawns real Chrome; `test_from_state` is
`#[doc(hidden)]` and needs a real `ChromeProcess`. To unit-test the manager state
machine without Chrome, introduce a narrow internal trait:

```rust
#[async_trait]
pub(crate) trait ManagedBrowser: Send + Sync {
    async fn resolved_port(&self) -> u16;
    async fn is_alive(&self) -> bool;
    async fn client(&self) -> anyhow::Result<CdpClient>;
    async fn stop(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait BrowserLauncher: Send + Sync {
    async fn launch(&self, config: BrowserConfig) -> anyhow::Result<Box<dyn ManagedBrowser>>;
}
```

Every method is `async` because the 0.2.0 accessors are (`is_alive`, `debug_address`,
`profile_dir`, `is_managed`, `pid` all became `async` to avoid the silent `try_lock`
fallbacks of 0.1.x). `RealBrowser` is a ~25-line newtype over `cdp_browser_lite::Browser`
whose `resolved_port` is `self.0.debug_address().await.1`.

The cost of this indirection is paid back by full unit coverage of the regression-prone
paths: restart with a changed proxy, port after dynamic reassignment, stop idempotency,
re-ensure after stop.

---

## 5. Phase plan

Every phase ends green (`cargo fmt`, `cargo clippy --all-targets --all-features -D warnings`,
`cargo test`). The `validator` subagent runs at the end of each phase.

### Phase 0 — Baseline

- Record current test count and `cargo test` output.
- No code changes.
- **Exit criterion:** documented baseline to compare against in Phase 6.

### Phase 1 — Trait evolution (pure refactor, still on `ChromeInstanceManager`)

Nothing about Chrome lifecycle changes yet; this is a behaviour-preserving refactor
that makes the seam ready.

1. **RED** — add `given_mock_manager_when_client_requested_then_connects_to_its_port`
   against the existing mock WebSocket server helper. Does not compile: `client()`
   doesn't exist.
2. **GREEN** — add `async fn client()` and make `stop_instance` async on the trait;
   implement for `ChromeInstanceManager` and `MockChromeManager`; move the
   `CdpClient::new` call out of `get_or_connect` into `manager.client()`; add `.await`
   at the two `stop_instance` call sites; remove `set_port`.
3. **REFACTOR** — none expected.

**Exit criterion:** all pre-existing tests pass unchanged. This phase is independently
revertable.

### Phase 2 — `LaunchPlan` (TDD, pure functions)

```rust
impl LaunchParams {
    pub(crate) fn plan(&self) -> LaunchPlan;
    fn to_config(&self, plan: &LaunchPlan) -> BrowserConfig; // adapter, untested
}
```

Mapping:

| Input | `LaunchPlan` field |
|---|---|
| host is local (`127.0.0.1`, `localhost`, `::1`) | `mode: LaunchMode::Auto` |
| host is remote | `mode: LaunchMode::AttachOnly` |
| `user_profile == true` | `profile: ProfileMode::UserDefault` |
| `user_profile == false` | `profile: ProfileMode::PersistentPerPort { root: std::env::temp_dir(), prefix: "chrome-mcp-profile-".into() }` |
| `enable_automation == false` | `extra_args: ["--disable-infobars"]` |
| `enable_automation == true` | `extra_args: []` (library emits `--enable-automation` **and** `--disable-infobars`) |

The mode split is mandatory, not cosmetic: `BrowserConfig::validate()` rejects `Auto`
and `LaunchNew` on non-local hosts, so a remote host must be `AttachOnly` or the config
fails to build.

Everything else maps onto builder calls in `to_config`: `.host()`, `.port()`,
`.headless()`, `.proxy()`, `.no_sandbox` left unset (the library's default resolution —
`headless || CHROME_NO_SANDBOX` — is byte-identical to today's, lines 309-311),
`.keep_alive_on_drop(false)` per D1, `.connect_timeout(10s)`, `.command_timeout(10s)`,
`.startup_timeout(10s)` (matches today's 50 × 200 ms).

`chrome_path` is left unset so `discovery::discover_default()` runs, which honours
`CHROME_PATH` with absolute priority and otherwise searches `PATH` plus per-OS
locations — a strict superset of today's three hardcoded paths.

**Tests:**

- `given_local_host_when_planning_then_mode_is_auto`
- `given_remote_host_when_planning_then_mode_is_attach_only`
- `given_user_profile_when_planning_then_profile_mode_is_user_default`
- `given_managed_profile_when_planning_then_profile_is_persistent_per_port`
- `given_persistent_per_port_plan_when_resolving_dir_then_matches_configured_prefix`
  (asserts the naming contract through the library's pure, public
  `ProfileMode::dir_for_port(port)` — this is what ties B1/B3 to a concrete directory)
- `given_automation_disabled_when_planning_then_disable_infobars_is_added`
- `given_automation_enabled_when_planning_then_no_extra_args`
- `given_plan_when_building_config_then_validate_succeeds` (guards against the
  `Auto` + remote-host and `Auto` + port-0 rejections)
- `given_proxy_set_when_building_config_then_port_accessor_matches_configured_port`
  (the only assertion available on `BrowserConfig` from outside)

### Phase 3 — `CdpBrowserManager` (TDD with fakes)

```rust
pub struct CdpBrowserManager {
    params: LaunchParams,
    launcher: Box<dyn BrowserLauncher>,
    browser: Option<Box<dyn ManagedBrowser>>,
    resolved_port: u16,
}
```

`ChromeManager` impl:

- `ensure_instance`: if the current browser reports `is_alive().await` → `Ok(())`.
  Otherwise `params.plan()` → `params.to_config(&plan)` → `launcher.launch(config)` →
  store the browser **and** cache `resolved_port = browser.resolved_port().await`.
- `stop_instance`: `if let Some(b) = self.browser.take() { b.stop().await?; }` —
  idempotent, `Ok(())` when already `None`. Resets `resolved_port` to `params.port`.
- `get_port`: returns the cached `resolved_port` (see 4.1).
- `set_proxy`: mutates `params.proxy` only. **It must not touch the live browser** —
  `BrowserConfig` is immutable, cannot be read back out of `Browser`, and
  `Browser::restart()` reuses the old config. `restart_chrome`'s existing
  stop → set_proxy → ensure sequence therefore already produces the correct result.
- `client`: `self.browser.as_ref().ok_or(...)?.client().await`.

**Tests** with `FakeLauncher` (records the `BrowserConfig::port()` it was handed, returns
a `FakeBrowser` with a scriptable `is_alive` and `resolved_port`):

- `given_no_browser_when_ensure_instance_then_launches_once`
- `given_live_browser_when_ensure_instance_then_does_not_relaunch`
- `given_dead_browser_when_ensure_instance_then_relaunches`
- `given_running_browser_when_stop_instance_then_browser_is_dropped`
- `given_no_browser_when_stop_instance_then_succeeds` (idempotency)
- `given_stopped_browser_when_ensure_instance_then_launches_fresh_instance` (B15)
- `given_reassigned_port_when_get_port_then_returns_resolved_port` (B14)
- `given_no_browser_when_get_port_then_returns_configured_port`
- `given_stopped_browser_when_get_port_then_returns_configured_port`
- `given_proxy_set_when_ensure_instance_then_config_carries_proxy` (B13)
- `given_no_browser_when_client_requested_then_errors`

### Phase 4 — Cut over and delete

1. `Cargo.toml`: add `cdp-browser-lite = "0.2.0"`, remove `cdp-lite`. Rewrite the
   `cdp_lite::` paths in `chrome_mcp_handler/mod.rs` (lines 31, 216, 219, 222, 225, 228,
   248) to `cdp_browser_lite::`. Verify with `cargo tree -d` that `cdp-lite` appears
   exactly once (0.2.0 still pins 0.1.3).
2. `ChromeMcpHandler::new_with_params` constructs `CdpBrowserManager`.
3. Delete `ChromeInstanceManager` and its now-dead helpers: `get_chrome_path`,
   `is_port_open`, `is_chrome_cdp`, `is_managed_profile_active`, `find_new_port`,
   `ensure_instance_impl`, `start_instance`, `patch_preferences`, `stop_instance_impl`,
   `log` / `log_to`.
4. Delete the unit tests covering that deleted logic (roughly `mod.rs:460-869`), keeping
   `MockChromeManager`. The scenarios they covered do not move into the MCP — they are
   now the library's responsibility and are re-asserted end-to-end in Phase 5.

**Note on `log_to`:** the file logger writes to `./logs/debug.log` relative to the CWD.
`cdp-browser-lite` uses the `tracing` facade instead. Dropping the ad-hoc logger is
intentional; if the diagnostics are wanted, add `tracing-subscriber` writing to stderr
in a follow-up — do not reintroduce the file logger.

### Phase 5 — Integration tests against real Chrome

Marked `#[ignore]`, run manually / in CI where Chrome is installed. These cover what the
fakes cannot.

**These carry more weight than usual.** `ProfileMode::PersistentPerPort` and
`BrowserPool` ship in 0.2.0 with **no upstream test coverage** (the identifier appears in
`src/` and `CHANGELOG.md` only, in zero test files). Until that is fixed upstream, the
B3 test below is the only executable verification anywhere that the profile directory
tracks the resolved port and that a live instance's `SingletonLock` survives.

- `given_real_chrome_when_ensure_and_client_then_browser_version_responds`
- `given_managed_instance_when_stop_instance_then_process_is_gone` (poll the port)
- `given_managed_instance_when_manager_dropped_then_process_is_gone` (D1 / B16)
- `given_attached_instance_when_stop_instance_then_process_survives` (B2)
- `given_managed_instance_on_configured_port_when_second_manager_ensures_then_uses_different_port_and_profile` (B3)
- `given_managed_instance_on_configured_port_when_second_manager_ensures_then_existing_singleton_lock_survives` (B3)

**Status on Chrome 151 (verified):**

- **B2 passes** with `cdp-browser-lite` 0.2.2 — the upstream `is_chrome_cdp` probe was fixed
  to use HTTP/1.1 (Chrome >= 151 ignores HTTP/1.0 requests).
- **B3 passes** with `cdp-browser-lite` 0.2.3 — the upstream `managed_lock_exists` was fixed to
  detect the `SingletonLock` via `symlink_metadata` instead of `.exists()`. On Chrome >= 151 the
  `SingletonLock` is a dangling symlink (target `<hostname>-<pid>` is never created; the real
  singleton is the socket under `/tmp/com.google.Chrome.*`), so `.exists()` returned false and
  the library fell through to `AttachAt`. With `symlink_metadata` a second managed instance
  correctly launches on a new port/profile.

`Cargo.toml` points at `cdp-browser-lite 0.2.4`. All six integration tests pass on Chrome 151.

### Phase 6 — Documentation and release

- `README.md`: document the D1 behaviour change (Chrome is now terminated when the MCP
  server exits) and the `cdp-browser-lite` dependency; refresh the troubleshooting
  section — executable discovery is now broader than `CHROME_PATH` plus three paths.
- Bump to `1.1.0` (behaviour change → minor, not patch).
- Compare the test count against the Phase 0 baseline and account for every removed test.
- Do **not** publish to crates.io or push to `master` without explicit approval.

---

## 6. Full BDD scenarios

Written in the `given_..._when_..._then_...` convention already used at
`chrome_instance/mod.rs:802`.

```gherkin
Feature: Chrome instance lifecycle

  Scenario: No Chrome running (B1)
    Given no process is listening on the configured port
    When the handler ensures an instance
    Then a managed Chrome is launched on that port
    And its user-data-dir is "chrome-mcp-profile-{port}"

  Scenario: Attach to a user-started Chrome (B2)
    Given a Chrome with remote debugging is listening on the configured port
    And no managed profile lock exists for that port
    When the handler ensures an instance
    Then no new process is spawned
    And the manager reports that port
    And stopping the instance leaves that Chrome running

  Scenario: Another managed instance occupies the port (B3)
    Given a Chrome with remote debugging is listening on the configured port
    And a managed profile lock exists for that port
    When the handler ensures an instance
    Then a new Chrome is launched on the next free port N
    And its user-data-dir is "chrome-mcp-profile-N"
    And the lock file of the pre-existing instance is untouched

  Scenario: Port squatted by a non-Chrome process, local host (B4)
    Given a non-CDP TCP listener occupies the configured port
    And the host is 127.0.0.1
    When the handler ensures an instance
    Then Chrome is launched on the next free port

  Scenario: Port squatted by a non-Chrome process, remote host (B5)
    Given a non-CDP TCP listener occupies the configured port
    And the host is not local
    When the handler ensures an instance
    Then it fails without attempting to spawn

  Scenario: Remote host unreachable (B6)
    Given the host is not local
    And nothing is listening on the configured port
    When the handler ensures an instance
    Then it fails without attempting to spawn

  Scenario: Restart with a proxy (B13)
    Given a managed instance is running
    When restart_chrome is called with proxy_server "http://p:8080"
    Then the previous instance is stopped
    And the new instance is launched with "--proxy-server=http://p:8080"
    And the cached CDP client has been invalidated

  Scenario: Port reassignment is visible to tools (B14)
    Given the manager launched Chrome on a port other than the configured one
    When a tool requests the CDP client
    Then the connection targets the actual port

  Scenario: Ensure after stop (B15)
    Given stop_chrome has been called
    When any tool triggers ensure_instance
    Then a fresh instance is launched

  Scenario: No zombies on exit (B16, D1)
    Given a managed Chrome is running
    When the manager is dropped
    Then the Chrome process is terminated
```

---

## 7. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| `PersistentPerPort` and `BrowserPool` ship untested upstream | **High** | B3 depends entirely on `PersistentPerPort`. The Phase 5 integration tests are currently its only coverage. Tracked upstream in `cdp-browser-lite/FOLLOWUP-001-auto-race-and-test-coverage.md`; land those tests before releasing 1.1.0. |
| Chrome killed on MCP exit surprises users relying on session persistence (D1) | Medium | Documented in `README.md`; minor version bump. Reversible by flipping one builder call. |
| Duplicate CDP listeners if `browser.client()` is called per tool invocation | Medium | `get_or_connect` keeps caching in `handler.client`; `manager.client()` is only called when that cache is empty. Note `Browser::client()` also costs a `Browser.getVersion` round-trip per call. |
| `Browser::drop` leaks the process when the state mutex is contended | Low | Inherent to the library (`browser.rs:403-419`). `stop_chrome` remains the deterministic path; `kill_on_drop(true)` on the Tokio child is the second net. |
| `LaunchMode::Auto` races between concurrent `ensure` calls | Low | `ensure_auto` builds a throwaway `PortAllocator` per call and drops the `PortReservation` before `spawn_managed` binds, so the in-process reservation does not protect anything. **No practical impact here**: `get_or_connect` holds the `chrome_manager` mutex around `ensure_instance`, so a single MCP process never issues concurrent `ensure` calls. Cross-process races between two MCP servers are unchanged from today. Tracked upstream in `FOLLOWUP-001`. |
| `--disable-infobars` silently dropped when automation is off (B10) | Low | Explicit `extra_args` entry, asserted in Phase 2. |
| Error messages change (`BrowserError` vs the current `anyhow!` strings) | Low | The `--user-profile` guidance message must be preserved: map `BrowserError::EarlyExit` to the existing wording in `CdpBrowserManager::ensure_instance` when `user_profile` is set. |
| Two `cdp-lite` versions compiled | Low | 0.2.0 pins 0.1.3, the version already in use; verify with `cargo tree -d`. |

---

## 8. Acceptance checklist

- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean, with **no** new
      `#[allow]` / `#[expect]` attributes
- [ ] `cargo test` green; test count reconciled against the Phase 0 baseline
- [ ] `cargo test -- --ignored` green on a machine with Chrome
- [ ] All 24 MCP tools still listed (`test_handle_list_tools_request` asserts 24)
- [ ] `cargo tree -d` shows a single `cdp-lite`
- [ ] `src/chrome_mcp_handler/chrome_instance/mod.rs` contains no process-spawning,
      port-probing, free-port-scanning, lock-file-inspecting or file-logging code
- [ ] `README.md` updated
- [ ] Version bumped to 1.1.0
- [ ] No push to `master`, no crates.io publish without explicit approval
