# Changelog

## [Unreleased]
### Chores
- Upgrade `rust-mcp-sdk` from `0.8.3` to `1.0.1`.

## [1.3.0]
### Fixes
- Documented the `WEB_MCP` and `WEBGL_SOFTWARE` capability presets and the previously undescribed boolean/array parameters (`headless`, `features`, `activate`, `clear`, `include_details`, `full_page`, `disable_cache`) directly in the tool-level `description` of `open_instance`, `restart_chrome`, `switch_tab`, `get_console_logs`, `get_network_logs`, `capture_screenshot`, and `profile_page_performance`. This is a workaround: the `JsonSchema` derive from `rust-mcp-macros` drops the `description` for `bool` and `Vec`/array fields, so those parameters were otherwise undiscoverable from the published schema.

### Known Tech Debt
- The `JsonSchema` derive from `rust-mcp-macros` drops the field `description` for `bool` and `Vec`/array fields (and does not emit `items.enum` for `Vec<ChromeFeature>`). The information is therefore folded into the tool descriptions as a stopgap. The proper fix — explicit input schemas, or migrating to `schemars` (which would also emit `items.enum` for the closed `ChromeFeature` set) — is deferred.

### Features
- Native support for controlling multiple concurrent tabs inside a single Chrome instance.
- Decoupled CDP event routing using a thread-safe `CdpTarget` enum (`Client` vs `Tab`) mapping back to the underlying `CdpClient` or `Tab`.
- Added 4 new tab management tools: `open_tab`, `list_tabs`, `close_tab`, and `switch_tab`.
- Tab and instance management tools (`open_tab`, `list_tabs`, `switch_tab`, `close_tab`, `open_instance`, `close_instance`) now return structured JSON (tab_id / instance_id payloads) instead of prose, so LLM clients can chain calls without parsing free text.
- Reworked tool descriptions for the instance and tab management tools into the standard MCP template (side effects, prerequisites, returns, use-this, alternatives), and documented LLM-facing rules: `switch_tab` changes the default target of subsequent calls, `close_instance` on 'default' stops but keeps the lazily re-created entry, and tools fall back to the single-tab connection when no tabs are registered.
- Server `instructions` now explain the instance/tab addressing model: `open_instance` `label` becomes the returned `instance_id`, and `tab_id` omission targets the active tab.
- Added optional `tab_id` parameter to all 22 page-scoped and interactive tools.
- Strict per-tab cache isolation for console messages, network activity, parsed scripts, WebMCP tools, and custom events.
- Real-time tab auto-discovery: newly created tabs or popups (via `window.open()`) are automatically discovered, attached, and registered.
- Robust event-pumping: listeners no longer exit silently when the underlying broadcast stream experiences lag, preventing silent failures under high-traffic multi-tab execution.
- Default Chrome profile is now **ephemeral**: a fresh profile is created per launch and removed on stop, so cookies, storage, and session state never bleed between MCP sessions, and the crash-restore bubble cannot appear. `--user-profile` is unchanged.
- `open_instance` now reports the real on-disk profile directory of the live browser (works with ephemeral profiles, whose path is randomly generated at launch).
- A second server on an occupied port now attaches to the existing CDP endpoint instead of relocating to a new port (no persistent per-port profile lock exists with ephemeral profiles); attached instances are never killed.

## [1.2.0]
### Features
- Spawns and manages multiple concurrent Chrome instances via a session registry and coordinated pool launcher.
- Added three new control tools: `open_instance`, `list_instances`, and `close_instance`.
- Updated all 28 existing tools to accept an optional `instance_id` parameter to target commands.
- Added CLI option `--max-instances` (default: 8) to limit concurrent instances.
- Added explicit diagnostic warning messages to `webmcp_list_tools` and related tools when WebMCP is disabled or pages haven't registered anything.
- Resolved a routing bug where 9 tools (`get_console_logs`, `search_scripts`, `evaluate_on_call_frame`, `get_custom_events`, `webmcp_list_tools`, `webmcp_list_invocations`, `webmcp_get_invocation`, `restart_chrome`, `stop_chrome`) ignored `instance_id` and defaulted to the main session.
- Features preset list is now dynamically tracked and updated in `list_instances` after a `restart_chrome`.
- Omitted `features` argument in `restart_chrome` now preserves the current features instead of silently wiping them.
- Populate standard MCP `instructions` parameter on initialization with clear multi-instance and WebMCP tips for LLMs.

## [1.1.1]
### Features
- Support for `WebMCP` and `WEBGL_SOFTWARE` capability presets in `restart_chrome`.
- Added WebMCP CDP domain tools (`webmcp_list_tools`, `webmcp_invoke_tool`, `webmcp_get_invocation`, `webmcp_list_invocations`).
### Chores
- Exclude IDE files and other caches in `.gitignore`.

## [1.1.0]
- Migrate Chrome lifecycle management to `cdp-browser-lite`.

## [1.0.10]
- Fix Chrome CDP rewrite.

## [1.0.9]
- Implement dynamic port management.

## [1.0.8]
- Fix WebGL support by removing `--disable-gpu`.

## [1.0.7]
- Improve tool descriptions for glama.ai quality standards.

## [1.0.6]
- Update `--user-profile` handling and default flags.

## [1.0.4]
- Add `--user-profile` flag.

## [1.0.3]
- Fix Chrome startup in Docker.

## [1.0.2]
- Fix broken test.

## [1.0.0]
- Synchronize version to 1.0.0.

*For earlier versions, see git tags.*