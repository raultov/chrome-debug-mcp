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
* **Isolated Profiles (Default)**: By default, every time the MCP server launches Chrome, it creates a **fresh, temporary user profile** in your system's temporary directory. This profile is completely independent of your main browser profile.
* **Incognito-like Experience**: No cookies, history, saved passwords, or session data from your personal accounts are shared with the managed instance by default.
* **Identity Protection**: Even if an LLM has full control over the browser, it cannot access your logged-in sessions (e.g., Google, GitHub, banking) or impersonate you unless explicitly authorized.
* **User Profile Mode**: Use the `--user-profile` flag to launch Chrome using your **existing system profile**. This is useful when you want the LLM to work within your active sessions (cookies, saved logins, etc.) without having to re-authenticate on every site. **Use with caution as this provides the LLM access to your personal browser data.**
  * ⚠️ **Note on `--user-profile`**: Due to Chrome's singleton architecture, if your browser is already open, it will delegate the request and **fail to open the debugging port**. You must either **close all existing Chrome instances** before starting the MCP, or start your browser manually with the `--remote-debugging-port=9222` flag.

**🚀 Chrome Instance Management**
* **Multi-Instance Support (New)**: Spawns and controls multiple concurrent, independent Chrome processes on dynamic ports, each with its own isolated profile directory. Limit the number of instances using the `--max-instances` flag.
* **Instance Registry Tools**: Use `open_instance`, `list_instances`, and `close_instance` to create, audit, and clean up additional instances. All existing tools accept an optional `instance_id` to route commands to the targeted browser.
* **Isolated Profiles**: Launches Chrome using a fresh, temporary profile by default, ensuring it doesn't share cookies, passwords, or session data with your main browser.
* **User Profile Support**: Optionally use `--user-profile` to leverage your existing browser sessions and cookies.
* **Dynamic Port Management**: Automatically detects if the default port (9222) is in use. 
  * If used by another managed `chrome-debug-mcp` instance, it **automatically finds a new available port** to avoid collisions.
  * If used by a user-started Chrome instance, it **automatically attaches** to it instead of spawning a new one.
* **Docker & Headless Support**: Full compatibility with Docker environments. Use the `--headless` flag to run Chrome without a GUI inside containers.

* **Remote/Host Connection**: Use the `--host` argument to connect to a Chrome instance running on a different machine or the host machine (e.g., `--host host.docker.internal` from inside a container).
* **Optional Automation Infobar**: Add the `--enable-automation` flag to explicitly show the native "Chrome is being controlled by automated test software" message. By default, this is disabled for stealthier interaction.
* **Proxy Support**: `restart_chrome` now accepts an optional `proxy_server` argument to launch Chrome routing traffic through a proxy.
* **Auto-Launch**: Automatically detects if Chrome is running on the specified port. If not, it spawns a new instance with the required flags.
* `restart_chrome`: Restarts the managed Chrome instance.
* **Capability Presets**: `restart_chrome` accepts an optional `features` array so a client can opt into extra browser capabilities per restart. It is a **closed set** — arbitrary Chrome flags are deliberately not accepted, to keep the tool from becoming a command line injection point:
  * `WEB_MCP` — enables the experimental WebMCP surface (`--enable-features=WebMCPTesting`, `--categoryExperimentalWebmcp=true`), for sites that expose tools to the browser.
  * `WEBGL_SOFTWARE` — forces SwiftShader software WebGL (`--use-gl=angle`, `--use-angle=swiftshader`, `--enable-unsafe-swiftshader`), for GPU-less environments such as containers.

  Presets apply to the instance started by that call; a later `restart_chrome` that omits `features` clears them, mirroring how `proxy_server` behaves.
* `stop_chrome`: Shuts down the managed Chrome instance gracefully (SIGTERM/SIGINT with fallback to SIGKILL).
* **Robust Lifecycle**: Fixed issues with dangling Chrome processes and patched preferences for cleaner restarts.
* **⚠️ Behaviour change**: Managed Chrome instances are now **terminated when the MCP server process exits** (including crashes). Previously a managed Chrome survived a server crash and was re-attached on restart; from now on it is killed. Attached (user-started) Chrome instances are never killed.

**🔐 Proxy Authentication**
* `enable_proxy_auth`: Automatically handles proxy authentication challenges by hooking into the `Fetch` CDP domain and supplying user-provided credentials (username & password).
* **Robustness Improvements**: Now features a 30-second timeout for slower residential proxies, and defaults to only intercepting `Document` requests to prevent breaking background requests.
* **Pre-warming**: Automatically navigates to a `prewarm_url` (defaults to `http://api.ipify.org?format=json`) to establish the proxy tunnel reliably before your main navigation task. You can optionally restrict the interception to a specific `resource_type`.

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

## ⚙️ Configuration

By default, the MCP Server discovers the Chrome executable through `cdp-browser-lite`'s cross-platform search: `CHROME_PATH` first (absolute priority), then common binaries in your `PATH` (`google-chrome`, `google-chrome-stable`, `chromium`, `chromium-browser`), then OS-specific locations (`/Applications/Google Chrome.app/...` on macOS, the `chrome.exe` install dir on Windows, `/usr/bin/google-chrome`, `/opt/google/chrome/chrome` and `/snap/bin/chromium` on Linux). This is a strict superset of the paths the server previously hardcoded.

**Arguments:**
* `--local`: Restricts navigation to local addresses only (`localhost`, `127.0.0.1`, `192.168.x.x`, or `*.local`). Highly recommended for security.
* `--headless`: Runs Chrome in headless mode (no GUI). Essential for Docker or server environments.
* `--user-profile`: Use the default system user profile (sessions, cookies, etc.) instead of a fresh one. This is useful for avoiding repeated logins during research sessions.
* `--host`: Specifies the target host for the Chrome instance (default: `127.0.0.1`). Use `host.docker.internal` to connect to a host machine from a container.
* `--port`: Specifies the remote debugging port (default: `9222`).
* `--enable-automation`: Enables the "controlled by automated software" infobar.
* `--max-instances`: Limits the maximum number of concurrent Chrome instances (default: 8). Ignored if `--user-profile` is set.

**Environment Variables:**
* `CHROME_PATH`: Explicitly define the path to the Chrome executable.

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

The easiest way to install and run the MCP Server natively is via Rust's Cargo or by downloading the pre-compiled binaries. You **do not** need to start Chrome manually anymore, the MCP Server will automatically launch a visible instance of Chrome with the correct debugging flags.

### 1. Installation

**Option A: Pre-compiled Binaries (Recommended)**
Go to the [Releases](https://github.com/raultov/chrome-debug-mcp/releases) page and download the native executable for your platform (macOS, Windows, Linux). We provide `.msi` installers for Windows and shell scripts for UNIX systems.

**Option B: Install via Cargo**
```bash
cargo install --git https://github.com/raultov/chrome-debug-mcp
```

**Option C: Install via Shell Script (Unix)**
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/raultov/chrome-debug-mcp/releases/latest/download/chrome-debug-mcp-installer.sh | sh
```

### 2. Configure your MCP Client
This server is fully tested and confirmed to work with **Claude Desktop**, **Gemini CLI**, and **ChatGPT (GPT) CLI**. Configure your AI client to execute the server using any of the following modes.

#### **Universal Configuration (JSON)**
Most MCP clients (like Claude Desktop or any JSON-based config) use this structure. Here are the three main usage modes:

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

#### **Gemini CLI**
To add and activate the server in Gemini CLI:
```bash
gemini mcp add chrome-debug-mcp chrome-debug-mcp
```
Then, inside the Gemini CLI session, enable it:
```bash
/mcp enable chrome-debug-mcp
```

### 3. Usage
Once connected, the AI agent will automatically handle starting Chrome when the first command is executed. The browser will remain visible so you can visually track the debugging process.

### 4. Agent Workflows & Multi-Instance Guidance

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

The resulting binary will be located in `target/release/chrome-debug-mcp`. This project utilizes `cargo-dist` to handle cross-platform native distribution seamlessly via GitHub Actions.

---

## 📖 Why this MCP Server?

Other integration servers like Puppeteer/Playwright wrappers are high-level, heavy, and typically fail at exposing **real, interactive step-by-step debuggers**. This MCP server uses raw CDP messages mapping them 1:1 to LLM tools, which allows intelligent agents to *literally* step over JS, read local scope variables natively, search inside V8 compiler contexts, and understand exactly why a script is crashing.

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for more details.