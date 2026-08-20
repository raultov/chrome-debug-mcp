# Changelog

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