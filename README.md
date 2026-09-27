# chrome-debug-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org)
[![chrome-debug-mcp MCP server](https://glama.ai/mcp/servers/raultov/chrome-debug-mcp/badges/score.svg)](https://glama.ai/mcp/servers/raultov/chrome-debug-mcp)

**chrome-debug-mcp** is an asynchronous Rust-based **Model Context Protocol (MCP)** server that allows AI agents and Large Language Models to natively control, automate, and debug Chromium-based browsers via the **Chrome DevTools Protocol (CDP)**.

Using [`cdp-browser-lite`](https://crates.io/crates/cdp-browser-lite) underneath (which itself re-exports the `cdp-lite` client), this MCP server directly hooks into the browser avoiding heavy abstractions, enabling live-debugging sessions directly from your editor or chat-interface. Starting from v0.2.0, it can also manage the Chrome process lifecycle automatically.

<div align="center">
  <a href="https://glama.ai/mcp/servers/raultov/chrome-debug-mcp">
    <img src="https://glama.ai/mcp/servers/raultov/chrome-debug-mcp/badges/card.svg" alt="chrome-debug-mcp MCP server" />
  </a>
</div>

---

## ✨ Features

This server natively implements a suite of tools categorized by CDP domains and native process management:

**🛡️ Privacy & Security**
* **Daemon Stability & Lifecycle (v1.5.0)**:
  * **Bounded Per-Navigation Retention**: `NetworkState` automatically caps network requests at 1,000 requests per navigation and retains up to 3 top-level navigations rotated on `Page.frameNavigated`. WebSocket frames are capped at 500 per connection, and console logs at 1,000 entries.
  * **Graceful Worker Teardown**: Event pump tasks are managed via RAII handles (`ListenerHandles`) and automatically aborted when tabs are closed or browser connections reset.
  * **Version Matching & Warnings**: `list_instances` reports detected `browser_version` data via `Browser.getVersion` and appends warnings if Chrome major version is below `120`.
  * **Actionable Validation**: ID lookup errors for tabs and instances list all active IDs and offer hints when a browser restart invalidates previous tab handles.
* **Isolated Profiles (Default)**: Every time the MCP server launches Chrome, it creates a **fresh, temporary user profile** in your system's temporary directory. This profile is completely independent of your main browser profile, and it is **removed when the browser stops** — cookies, history, saved passwords, or session data from one session never bleed into the next.
* **Incognito-like Experience**: No cookies, history, saved passwords, or session data from your personal accounts are shared with the managed instance by default.
* **Identity Protection**: Even if an LLM has full control over the browser, it cannot access your logged-in sessions (e.g., Google, GitHub, banking) or impersonate you unless explicitly authorized.
* **Cookie Import (`--allow-cookie-import`)**: When started with the `--allow-cookie-import` flag, the server allows tools (`navigate`, `open_instance`, `restart_chrome`) to accept `copy_cookies: true` to seed the isolated ephemeral profile with your real Chrome session cookies.
  * **Opt-in & Human Consent**: Off by default. The LLM must explicitly ask the user for confirmation before setting `copy_cookies: true`.
  * **Cross-Platform Compatibility**:
    * **Linux**: Supported (AES-128-CBC `v11` decrypted via desktop keyring). Imports `os_crypt.selected_backend` to prevent silent decryption failures.
    * **macOS**: Supported (Keychain `v10` decrypted via Chrome binary ACL).
    * **Windows**: Supported (`v10`/`v20` via DPAPI/App-Bound Encryption in `os_crypt`).
  * **Destructive Relaunch Guard**: If `navigate` is called with `copy_cookies: true` on an already-running instance, it returns an error detailing all open tabs and requires `confirm_restart: true` before restarting the instance with the seeded cookies.
  * **Security**: Password databases (`Login Data`) are **never copied**.
* **User Profile Mode**: Use the `--user-profile` flag to launch Chrome using your **existing system profile**. This is useful when you want the LLM to work within your active sessions (cookies, saved logins, etc.) without having to re-authenticate on every site. **Use with caution as this provides the LLM access to your personal browser data.**
  * ⚠️ **Note on `--user-profile`**: Due to Chrome's singleton architecture, if your browser is already open, it will delegate the request and **fail to open the debugging port**. You must either **close all existing Chrome instances** before starting the MCP, or start your browser manually with the `--remote-debugging-port=9222` flag.

**🚀 Chrome Instance & Tab Management**
* **Multi-Instance Support**: Spawns and controls multiple concurrent, independent Chrome processes on dynamic ports, each with its own isolated profile directory. Limit the number of instances using the `--max-instances` flag.
* **Instance Registry Tools**: Use `open_instance`, `list_instances`, and `close_instance` to create, audit, and clean up additional instances. All existing tools accept an optional `instance_id` to route commands to the targeted browser.
* **Multi-Tab Support (New)**: Controls multiple concurrent tabs within a single Chrome instance, multiplexing the event streams and commands over a single WebSocket connection.
  * **Auto-Discovery**: Popups opened by target pages (e.g. `window.open()`) are automatically discovered, attached, and registered in the session's tab registry.
  * **Cache Isolation**: State caches (console messages, network traffic, debugger parsed scripts, WebMCP tools) are strictly isolated per tab so events do not bleed across targets.
* **Tab Registry Tools (New)**:
  * `open_tab` — Opens a new tab, optionally with a custom label and target URL. Returns JSON with the `tab_id` to reuse in other tools.
  * `list_tabs` — Lists all open and registered tabs for the instance as JSON (`tab_id`, `label`, `target_id`, `url`) plus the currently active tab. When no tabs are registered, tools fall back to the instance's default single-tab connection.
  * `close_tab` — Closes a specific tab by ID and cleans up its cache state. Returns the new active tab.
  * `switch_tab` — Changes the default active tab used when `tab_id` is omitted in tool calls, and optionally brings it to the foreground.
* **LLM-Friendly Interface**: The lifecycle tools (`open_instance`, `close_instance`, `open_tab`, `list_tabs`, `switch_tab`, `close_tab`) return structured JSON so agents can chain calls without regex-parsing prose, and their descriptions follow the standard MCP template (side effects, prerequisites, returns, alternatives) so models rank them correctly.
* **Target Routing (New)**: All tab-scoped tools accept an optional `tab_id` parameter to target commands and retrieve cache state from a specific tab. If omitted, the default active tab is targeted.
* **Isolated Profiles**: Launches Chrome using a fresh, temporary profile by default, ensuring it doesn't share cookies, passwords, or session data with your main browser.
* **User Profile Support**: Optionally use `--user-profile` to leverage your existing browser sessions and cookies.
* **Dynamic Port Management**: Automatically detects if the default port (9222) is in use. 
  * If the port is occupied by a Chrome instance exposing CDP (user-started or another managed `chrome-debug-mcp` instance), it **automatically attaches** to it instead of spawning a new one.
  * Managed profiles are ephemeral, so there is no persistent per-port state; a second server sharing a port simply shares the same browser (and never kills an attached instance).
* **Docker & Headless Support**: Full compatibility with Docker environments. Use the `--headless` flag to run Chrome without a GUI inside containers.

* **Remote/Host Connection**: Use the `--host` argument to connect to a Chrome instance running on a different machine or the host machine (e.g., `--host host.docker.internal` from inside a container).
* **Optional Automation Infobar**: Add the `--enable-automation` flag to explicitly show the native "Chrome is being controlled by automated test software" message. By default, this is disabled for stealthier interaction.
* **Proxy Support**: `restart_chrome` now accepts an optional `proxy_server` argument to launch Chrome routing traffic through a proxy.
* **Auto-Launch**: Automatically detects if Chrome is running on the specified port. If not, it spawns a new instance with the required flags.
* `restart_chrome`: Restarts the managed Chrome instance.
* **Capability Presets**: `restart_chrome` accepts an optional `features` array so a client can opt into extra browser capabilities per restart. It is a **closed set** — arbitrary Chrome flags are deliberately not accepted, to keep the tool from becoming a command line injection point:
  * `WEB_MCP` — enables the experimental WebMCP surface (`--enable-features=WebMCPTesting,DevToolsWebMCPSupport`), for sites that expose tools to the browser.
  * `WEBGL_SOFTWARE` — forces SwiftShader software WebGL (`--use-gl=angle`, `--use-angle=swiftshader`, `--enable-unsafe-swiftshader`), for GPU-less environments such as containers.

  Presets apply to the instance started by that call; a later `restart_chrome` that omits `features` clears them, mirroring how `proxy_server` behaves.
* `stop_chrome`: Shuts down the managed Chrome instance gracefully (SIGTERM/SIGINT with fallback to SIGKILL).
* **Robust Lifecycle**: Fixed issues with dangling Chrome processes. Ephemeral profiles are deleted on stop, and `cdp-browser-lite` sweeps orphaned profile dirs left behind by abrupt kills; the "Chrome didn't shut down correctly" restore bubble is suppressed via launch flags and profile patching.
* **⚠️ Behaviour change**: Managed Chrome instances are now **terminated when the MCP server process exits** (including crashes). Previously a managed Chrome survived a server crash and was re-attached on restart; from now on it is killed. Attached (user-started) Chrome instances are never killed.

**🔐 Proxy Authentication**
* `enable_proxy_auth`: Automatically handles proxy authentication challenges by hooking into the `Fetch` CDP domain and supplying user-provided credentials. Exposed only when the server is started with `--proxy-server`.
* **Robustness Improvements**: Features a 30-second timeout for slower residential proxies, and defaults to only intercepting `Document` requests to prevent breaking background requests.
* **Pre-warming & Credentials**: Pre-warms proxy connections via `http://api.ipify.org?format=json`. Can use credentials passed via `--proxy-username`/`--proxy-password` or in tool arguments.

**🖱️ User Input**
* `click_element`: Simulates a native mouse click on a specific element by using a CSS selector. It calculates the center coordinates of the element and dispatches CDP mouse events directly.
* `fill_input`: Fills an input field in the DOM with specified text. It focuses the element via CSS selector and then uses native CDP `Input.insertText`.
* `scroll`: Scrolls the page by pixels, viewport heights (pages), or to a specific element. Essential for interacting with lazy-loaded content or infinite scrolling.

**📡 Network Inspection**
* `get_network_logs`: Retrieve intercepted network requests (REST/HTTP) and WebSocket frames.
* **Advanced Filtering**: Filter logs by URL, resource type, WebSocket direction, or payload content.
* **Payload Inspection**: Access full request/response headers, REST response bodies, and WebSocket frames.
* **Context Optimized**: Optional "summary mode" to avoid flooding the LLM context window.

**🪵 Console & Errors**
* `get_console_logs`: Retrieve console logs from the browser. This includes console.log/warn/error calls, exceptions, and network errors. Crucial for troubleshooting page scripts and errors. Includes optional log level filtering and a `clear` flag to manage state efficiently.

**⚡ Performance & Profiling**
* `get_performance_metrics`: Retrieve run-time performance metrics from the browser (e.g., JS heap size, DOM nodes, layout duration). Useful for getting a quick snapshot of the page's memory and computational overhead.
* `profile_page_performance`: Record and analyze a performance trace of the page. It automatically calculates Core Web Vitals (FCP, LCP, DCL, Load) and identifies the top Long Tasks (main thread blocking operations). You can optionally reload the page with cache disabled to simulate a cold start.

**🌐 Page & Runtime Control**
* `capture_screenshot`: Take a screenshot of the current page (or full page layout) and return it to the LLM client as a base64 encoded image block.
* `navigate`: Navigate the active tab to a specific URL.
* `reload`: Reload the current page.
* `inspect_dom`: Fetch the entire HTML or a smart snippet around a search query.
  * **Context Search**: Search for specific text and get a configurable number of characters around it.
  * **Token Efficiency**: Drastically reduce context window usage for large pages.
* `evaluate_js`: Run an arbitrary JavaScript expression globally on the page context.

**🐞 Live Debugging & Execution Control**
* `pause_on_load`: Enables the debugger and triggers a page reload, pausing execution on the very first parsed script statement.
* `search_scripts`: Search across all parsed script contexts for a query to accurately find lines and columns for breakpoints.
* `set_breakpoint`: Set a precise JS breakpoint using `script_id`, `url`, or exact `script_hash`.
* `evaluate_on_call_frame`: Evaluate a JavaScript expression directly inside the *local scope* of the currently paused debugger call frame.
* `step_over`: Step over the next expression line.
* `resume`: Unpause and resume the execution.
* `remove_breakpoint`: Remove a previously set breakpoint.

**🧩 WebMCP (page-exposed tools)**
Requires restarting Chrome with the `WEB_MCP` capability preset (see `restart_chrome`).
* `webmcp_list_tools`: Lists the tools the current page exposes to the browser (name, description, `inputSchema`, `frameId`).
* `webmcp_invoke_tool`: Invokes a page tool by name. `input` is a **JSON object string** (e.g. `"{}"` or `"{\"product\":\"knot\"}"`), matching the tool's `inputSchema`. Blocks up to 30s waiting for the result.
* `webmcp_get_invocation`: Returns the status (`Pending`/`Completed`/`Error`/`Canceled`) and result of an invocation by `invocationId` — non-blocking.
* `webmcp_list_invocations`: Lists all invocations in the session with their status, with optional `status` filter.

  ⚠️ **Consent dialogs**: page tools with side effects (clipboard writes, form submissions…) may show an on-page confirmation dialog that a human must click. In that case `webmcp_invoke_tool` returns a timeout error containing the `invocationId` — the invocation stays `Pending` (it is NOT canceled), so you can poll it with `webmcp_get_invocation` after the user approves or denies it.

**🧪 Stability & Reliability**
* **Extensive Unit Testing**: Comprehensive test suite ensuring the reliability of event processing and tool deserialization, particularly in the `debugger` domain.
* **Side-Effect Free Tests**: All unit tests are designed to run in isolation, without launching real Chrome instances or modifying the filesystem.
* **Internal Refactoring**: Decoupled core logic through traits and dependency injection to ensure long-term maintainability.

---

## 📦 Installation

Every method installs the **same pre-compiled binaries** published on each [GitHub Release](https://github.com/raultov/chrome-debug-mcp/releases); a Rust toolchain is only needed for the Cargo and source routes. The npm packages are small wrappers whose `postinstall` fetches the pre-compiled binary for your platform from GitHub Releases — no compiling, no Rust toolchain.

Prefer zero effort? The one-prompt route below lets your AI agent do all of it. Otherwise, methods are grouped by platform and ordered by popularity.

### 🤖 One-Prompt Install (any platform)

Paste this prompt into any AI coding agent's chat (Claude Code, opencode, Cursor, Codex CLI, Copilot, ...) and let it install the server and register it with every MCP-capable client it detects on your machine:

```text
Install and set up the chrome-debug-mcp MCP server on this machine. Do not launch Chrome yourself — the server launches and manages it automatically.

1. Install the server binary with the first method that works on this OS:
   - Homebrew available (macOS/Linux): brew install raultov/tap/chrome-debug-mcp
   - Node.js >= 14 available (any OS): npm install -g @raultov/chrome-debug-mcp
   - macOS/Linux fallback: curl --proto '=https' --tlsv1.2 -LsSf https://github.com/raultov/chrome-debug-mcp/releases/latest/download/chrome-debug-mcp-installer.sh | sh
   - Windows fallback: download and run the .msi from https://github.com/raultov/chrome-debug-mcp/releases/latest
   Verify with: chrome-debug-mcp --version (add the install dir to PATH if it is not found).

2. Register the MCP server with every AI client you detect on this machine:
   - Claude Code: claude mcp add --scope user chrome-debug-mcp chrome-debug-mcp
   - opencode (and agy, its bridge): merge into ~/.config/opencode/opencode.json —
     "mcp": { "chrome-debug-mcp": { "type": "local", "command": ["chrome-debug-mcp"] } }
   - Codex CLI: append to ~/.codex/config.toml —
     [mcp_servers.chrome-debug-mcp]
     command = "chrome-debug-mcp"
   - Any other MCP-capable client (Cursor, Windsurf, VS Code, ...): add it with its own MCP settings, command "chrome-debug-mcp" with no arguments.

3. Report what you installed and which clients you configured, and remind me to restart each client so the MCP server loads. Do not enable --user-profile or cookie import unless I ask for them.
```

### macOS

**1. Homebrew** — the natural option on macOS:
```bash
brew install raultov/tap/chrome-debug-mcp
```
The formula fetches the pre-compiled binary (Apple Silicon & Intel) straight from GitHub Releases, and `brew upgrade` keeps you current on every release.

**2. npm / npx** — the MCP-client convention (requires Node.js >= 14):
```bash
npm install -g @raultov/chrome-debug-mcp   # installs the `chrome-debug-mcp` command
npx -y @raultov/chrome-debug-mcp           # ...or just run it once, nothing to install
```

**3. Shell installer** — one-liner that installs to `~/.cargo/bin`:
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/raultov/chrome-debug-mcp/releases/latest/download/chrome-debug-mcp-installer.sh | sh
```

**4. Cargo** — if you already have Rust:
```bash
cargo install chrome-debug-mcp     # compiles from crates.io
cargo binstall chrome-debug-mcp    # with cargo-binstall: fetches the pre-compiled binary, no compile
```

**5. Manual download** — get the `aarch64-apple-darwin` or `x86_64-apple-darwin` `.tar.xz` archive from the [Releases](https://github.com/raultov/chrome-debug-mcp/releases) page.

### Linux

**1. npm / npx** — the MCP-client convention (requires Node.js >= 14):
```bash
npm install -g @raultov/chrome-debug-mcp   # installs the `chrome-debug-mcp` command
npx -y @raultov/chrome-debug-mcp           # ...or just run it once, nothing to install
```

**2. Shell installer** — one-liner that installs to `~/.cargo/bin`:
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/raultov/chrome-debug-mcp/releases/latest/download/chrome-debug-mcp-installer.sh | sh
```
The Linux binaries require glibc >= 2.35 (Ubuntu 22.04+, Debian 12+, Fedora 36+).

**3. Homebrew (Linuxbrew)** — if you already use [Homebrew-on-Linux](https://docs.brew.sh/Homebrew-on-Linux):
```bash
brew install raultov/tap/chrome-debug-mcp
```

**4. Cargo** — if you already have Rust:
```bash
cargo install chrome-debug-mcp     # compiles from crates.io
cargo binstall chrome-debug-mcp    # with cargo-binstall: fetches the pre-compiled binary, no compile
```

**5. Manual download** — get the `x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` `.tar.xz` archive from the [Releases](https://github.com/raultov/chrome-debug-mcp/releases) page.

### Windows

**1. npm / npx** — the MCP-client convention (requires Node.js >= 14):
```powershell
npm install -g @raultov/chrome-debug-mcp   # installs the `chrome-debug-mcp` command
npx -y @raultov/chrome-debug-mcp           # ...or just run it once, nothing to install
```

**2. MSI installer** — the native Windows experience: download `chrome-debug-mcp-x86_64-pc-windows-msvc.msi` from the [Releases](https://github.com/raultov/chrome-debug-mcp/releases) page and double-click it.

**3. PowerShell installer** — one-liner:
```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/raultov/chrome-debug-mcp/releases/latest/download/chrome-debug-mcp-installer.ps1 | iex"
```

**4. Cargo** — if you already have Rust:
```powershell
cargo install chrome-debug-mcp     # compiles from crates.io
cargo binstall chrome-debug-mcp    # with cargo-binstall: fetches the pre-compiled binary, no compile
```

**5. Manual download** — get the `x86_64-pc-windows-msvc` `.zip` archive from the [Releases](https://github.com/raultov/chrome-debug-mcp/releases) page.

### Docker & Cloud

For containerized isolation, host-hybrid mode, and the zero-install Glama cloud deployment, see the **Docker & Headless Usage** section below.

### From Source

See **Compilation (From Source)** below.

---

## ⚙️ Configuration

The MCP Server discovers the Chrome executable using a cross-platform search: `CHROME_PATH` environment variable first (absolute priority), followed by common binaries in `PATH` (`google-chrome`, `google-chrome-stable`, `chromium`, `chromium-browser`), and finally standard OS install paths (`/Applications/Google Chrome.app/...` on macOS, `chrome.exe` locations on Windows, `/usr/bin/google-chrome`, `/opt/google/chrome/chrome`, and `/snap/bin/chromium` on Linux).

---

### 1. MCP Server Startup Flags (CLI)

These flags configure the MCP server process when launched. Pass them on the command line when starting `chrome-debug-mcp` (or in your client's `args` / `command` definition).

| Flag | Description | Default | Possible Values / Format |
|---|---|---|---|
| `--local` | Restricts navigation to local addresses only (`localhost`, `127.0.0.1`, `192.168.x.x`, `*.local`). Highly recommended for security. | `off` | Flag present (`on`) or omitted (`off`) |
| `--enable-automation` | Shows the native "Chrome is being controlled by automated test software" infobar. | `off` | Flag present (`on`) or omitted (`off`) |
| `--user-profile` | Uses your default system Chrome profile (cookies, saved logins) instead of a fresh, isolated temporary profile. | `off` | Flag present (`on`) or omitted (`off`) |
| `--allow-cookie-import` | Exposes cookie-import parameters (`copy_cookies`, `source_profile`, `confirm_restart`) to tools (`navigate`, `open_instance`, `restart_chrome`). | `off` | Flag present (`on`) or omitted (`off`) |
| `--proxy-server <URL>` | Configures proxy server for Chrome instances and exposes proxy tools (`enable_proxy_auth`). Alias: `--proxy`. | *(none)* | Valid proxy URL (e.g. `http://proxy.example.com:8080`) |
| `--proxy-username <USER>` | Default username for proxy authentication. | *(none)* | String username |
| `--proxy-password <PASS>` | Default password for proxy authentication. | *(none)* | String password |
| `--headless` | Runs Chrome in headless mode (no GUI). Required for GPU-less or Docker environments. | `off` | Flag present (`on`) or omitted (`off`) |
| `--host <HOST>` | Target host IP address for Chrome remote debugging connection. | `127.0.0.1` | Valid IP address (e.g. `127.0.0.1`, `host.docker.internal`) |
| `--port <PORT>` | Chrome remote debugging port for the primary instance. | `9222` | Any free TCP port (`1`–`65535`) |
| `--max-instances <N>` | Maximum number of concurrent independent Chrome instances allowed in the instance pool. Ignored if `--user-profile` is set. | `8` | Positive integer (e.g. `1`, `4`, `16`) |

---

### 2. Environment Variables

| Variable | Description | Default | Value Format |
|---|---|---|---|
| `CHROME_PATH` | Explicit absolute path to the Chrome or Chromium binary. Overrides all automatic binary discovery paths. | *(not set)* | Absolute file path (e.g. `/usr/bin/google-chrome-stable` or `C:\Program Files\Google\Chrome\Application\chrome.exe`) |

---

### 3. Chrome Instance Launch Switches & Presets

When the MCP server spawns Chrome, it constructs switches based on its startup flags, dynamically allocated ports, and capability presets requested per instance (e.g. via `open_instance` or `restart_chrome`).

#### A. Server-to-Chrome Flag Mapping

| Server Input / State | Chrome Command Line Switch(es) Applied | Effect / Description |
|---|---|---|
| `--port <PORT>` | `--remote-debugging-port=<PORT>` | Binds V8 Inspector CDP WebSocket endpoint to the specified port. |
| `--user-profile` omitted *(default)* | `--user-data-dir=<TMP_DIR>` | Creates an isolated, ephemeral profile directory in `/tmp` deleted automatically on shutdown. |
| `--user-profile` passed | *(no `--user-data-dir` switch)* | Delegates to system default profile location (`~/.config/google-chrome`, Keychain/DPAPI). |
| `--headless` passed | `--headless` | Runs browser without GUI. |
| `--enable-automation` omitted *(default)* | `--disable-infobars` | Suppresses the "controlled by automated software" notification bar for stealthier interaction. |
| `--enable-automation` passed | *(no `--disable-infobars` switch)* | Shows native automation infobar. |
| `proxy_server` parameter *(dynamic)* | `--proxy-server="<PROXY_URL>"` | Routes instance network traffic through the specified HTTP/SOCKS proxy. |
| Seed profile / Cookie import *(dynamic)* | `--user-data-dir=<SEEDED_TMP_DIR>` | Copies decrypted Chrome cookies into a fresh ephemeral profile copy. |

#### B. Capability Presets (`features` parameter)

Dynamic tools (`open_instance`, `restart_chrome`) accept a `features` array of closed capability presets:

| Preset Name | Chrome Command Line Switches Applied | Use Case / Purpose |
|---|---|---|
| `"WEB_MCP"` | `--enable-features=WebMCPTesting,DevToolsWebMCPSupport` | Enables the experimental WebMCP page-exposed tools surface (API & DevTools inspection). |
| `"WEBGL_SOFTWARE"` | `--use-gl=angle`<br>`--use-angle=swiftshader`<br>`--enable-unsafe-swiftshader` | Forces SwiftShader software rasterization for WebGL in GPU-less containers. |

---

## 🐳 Docker & Headless Usage (v1.0.0)

`chrome-debug-mcp` is fully container-ready. This allows several powerful use cases for LLMs:

### 1. Cloud Deployment (via Glama)
The easiest way to use this server. Glama spawns a Docker container with Chrome pre-installed. The LLM gets immediate access to a browser in the cloud without any local setup.

### 2. Isolated Local Use
Run everything inside Docker to avoid installing Chrome or Rust on your host machine:
```bash
docker build -t chrome-mcp .
docker run -i --rm chrome-mcp --headless
```

### 3. Hybrid Mode (Container controlling Host)
The MCP server runs inside a secure Docker container but controls the Chrome instance on your actual desktop. This allows the LLM to assist you in your real browsing session:
1. Start your local Chrome with: `--remote-debugging-port=9222`
   * *Note: If you need proxy support in this mode, you must also start Chrome with the `--proxy-server="http://your-proxy:port"` flag.*
2. Run the container:
```bash
# On macOS/Windows
docker run -i --rm chrome-mcp --host host.docker.internal
```

---

## 🚀 Quick Start

Pick any installation method above, then point your MCP client at the `chrome-debug-mcp` command. You **do not** need to start Chrome manually anymore, the MCP Server will automatically launch a visible instance of Chrome with the correct debugging flags.

### 1. Configure your MCP Client
This server is fully tested and confirmed to work with **Claude Code**, **agy**, and **codex**. Configure your AI client to execute the server using any of the following modes.

#### **Universal Configuration (JSON)**
Most MCP clients (like Claude Code or any JSON-based config) use this structure. Here are the three main usage modes:

```json
{
  "mcpServers": {
    "chrome-debug-mcp": {
      "command": "chrome-debug-mcp",
      "args": [],
      "env": {}
    },
    "chrome-docker": {
      "command": "docker",
      "args": ["run", "-i", "--rm", "chrome-debug-mcp:v1.0.9", "--headless"]
    },
    "chrome-docker-hybrid": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "--net=host",
        "chrome-debug-mcp:v1.0.9",
        "--host",
        "127.0.0.1"
      ]
    }
  }
}
```
*Note: The `chrome-docker-hybrid` mode using `--net=host` is the recommended way on Linux to allow the container to access your local Chrome instance on `127.0.0.1`.*

#### **Claude Code**
To add and activate the server in Claude Code:
```bash
claude mcp add chrome-debug-mcp chrome-debug-mcp
# ...or without installing anything first (Node.js >= 14):
claude mcp add chrome-debug-mcp -- npx -y @raultov/chrome-debug-mcp
```

### 2. Usage
Once connected, the AI agent will automatically handle starting Chrome when the first command is executed. The browser will remain visible so you can visually track the debugging process.

### 3. Agent Workflows & Multi-Instance Guidance

LLMs can operate this server using a few optimized patterns:

#### A. Isolated Multi-Instance Scenarios
When running automated browser sessions, you can launch separate Chrome processes to prevent cookie pollution or tab collision:
1. Call `open_instance` with `label: "user-session-1"` or optional proxy server configs. This returns a unique `instance_id` (e.g. `chrome-2`).
2. Pass the `instance_id` explicitly to downstream tools like `navigate`, `evaluate_js`, or `webmcp_list_tools`.
3. Clear up resources using `close_instance` once finished.

#### B. Working with WebMCP
If you navigate to a page that supports WebMCP (e.g., https://www.knot.kz/#/agent-tools):
1. Tools registered by the web page can be retrieved using `webmcp_list_tools`.
2. By default, `WEB_MCP` is disabled for safety. If the tools list is empty, call `restart_chrome` with `features: ["WEB_MCP"]` and then `reload`.
3. Invoke page tools using `webmcp_invoke_tool`, providing input JSON arguments. If a consent dialog pauses execution on the web page, the tool will timeout after 30 seconds but keep the invocation pending. You can poll its result using `webmcp_get_invocation`.

---

## 🛠 Compilation (From Source)

If you wish to compile from source:

```bash
git clone https://github.com/raultov/chrome-debug-mcp
cd chrome-debug-mcp
cargo build --release
```

### Development & Code Quality

```bash
make check                                  # Run all local quality gates (fmt, clippy, test, dupes)

# Or run gates individually:
cargo clippy --all-targets -- -D warnings  # Must pass
cargo fmt -- --check                        # Must pass
cargo test --all-targets                    # Run unit tests
cargo dupes check                           # Code duplication check
```

The resulting binary will be located in `target/release/chrome-debug-mcp`. This project utilizes `cargo-dist` to handle cross-platform native distribution seamlessly via GitHub Actions.

---

## 📖 Why this MCP Server?

Other integration servers like Puppeteer/Playwright wrappers are high-level, heavy, and typically fail at exposing **real, interactive step-by-step debuggers**. This MCP server uses raw CDP messages mapping them 1:1 to LLM tools, which allows intelligent agents to *literally* step over JS, read local scope variables natively, search inside V8 compiler contexts, and understand exactly why a script is crashing.

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for more details.