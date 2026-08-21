# Changelog

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