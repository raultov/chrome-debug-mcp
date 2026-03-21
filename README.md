# chrome-debug-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org)
[![chrome-debug-mcp MCP server](https://glama.ai/mcp/servers/raultov/chrome-debug-mcp/badges/score.svg)](https://glama.ai/mcp/servers/raultov/chrome-debug-mcp)

**chrome-debug-mcp** is an asynchronous Rust-based **Model Context Protocol (MCP)** server that allows AI agents and Large Language Models to natively control, automate, and debug Chromium-based browsers via the **Chrome DevTools Protocol (CDP)**.

Using `cdp-lite` underneath, this MCP server directly hooks into the browser avoiding heavy abstractions, enabling live-debugging sessions directly from your editor or chat-interface. Starting from v0.2.0, it can also manage the Chrome process lifecycle automatically.

<div align="center">
  <a href="https://glama.ai/mcp/servers/raultov/chrome-debug-mcp">
    <img src="https://glama.ai/mcp/servers/raultov/chrome-debug-mcp/badges/card.svg" alt="chrome-debug-mcp MCP server" />
  </a>
</div>

---

## ✨ Features (v1.0.3)

This server natively implements a suite of tools categorized by CDP domains and native process management:

**🔒 Local-Only Mode (v0.9.3)**
* **Restricted Navigation**: Run the MCP server with the `--local` argument to restrict navigation to local addresses only: `localhost`, `127.0.0.1`, `192.168.x.x`, or addresses with the `.local` suffix. This is ideal for securely debugging local development environments without risking accidental navigation to external sites.
* **Clear Error Messaging**: If a navigation to an external address is attempted in local-only mode, the server returns a descriptive error explaining the restriction and how to disable it.

**🛠️ Custom CDP Commands (v0.9.0)**
* `send_cdp_command`: **EXPERIMENTAL**. Send any raw CDP command directly to the browser. This serves as a powerful fallback for any domain or command not yet natively implemented in specialized tools.
* `get_custom_events`: Retrieve a list of events captured from the browser that are not handled by other specialized listeners (like network or console). Essential for observing the side-effects of custom commands.
* **Broad Event Capture**: Automatically captures events from over 20+ domains (Target, DOM, CSS, Storage, etc.) and stores them in a rolling buffer for later inspection.

**🚀 Chrome Instance Management (v1.0.0)**
* **Docker & Headless Support**: Full compatibility with Docker environments. Use the `--headless` flag to run Chrome without a GUI inside containers.
* **Remote/Host Connection**: Use the `--host` argument to connect to a Chrome instance running on a different machine or the host machine (e.g., `--host host.docker.internal` from inside a container).
* **Optional Automation Infobar**: Add the `--enable-automation` flag to explicitly show the native "Chrome is being controlled by automated test software" message. By default, this is disabled for stealthier interaction.
* **Proxy Support**: `restart_chrome` now accepts an optional `proxy_server` argument to launch Chrome routing traffic through a proxy.
* **Auto-Launch**: Automatically detects if Chrome is running on the specified port. If not, it spawns a new instance with the required flags.
* `restart_chrome`: Restarts the managed Chrome instance.
* `stop_chrome`: Shuts down the managed Chrome instance gracefully (SIGTERM/SIGINT with fallback to SIGKILL).
* **Robust Lifecycle**: Fixed issues with dangling Chrome processes and patched preferences for cleaner restarts.

**🔐 Proxy Authentication (v0.8.0)**
* `enable_proxy_auth`: Automatically handles proxy authentication challenges by hooking into the `Fetch` CDP domain and supplying user-provided credentials (username & password).
* **Robustness Improvements**: Now features a 30-second timeout for slower residential proxies, and defaults to only intercepting `Document` requests to prevent breaking background requests.
* **Pre-warming**: Automatically navigates to a `prewarm_url` (defaults to `http://api.ipify.org?format=json`) to establish the proxy tunnel reliably before your main navigation task. You can optionally restrict the interception to a specific `resource_type`.

**🖱️ User Input (v0.5.1)**
* `click_element`: Simulates a native mouse click on a specific element by using a CSS selector. It calculates the center coordinates of the element and dispatches CDP mouse events directly.
* `fill_input`: Fills an input field in the DOM with specified text. It focuses the element via CSS selector and then uses native CDP `Input.insertText`.
* `scroll`: Scrolls the page by pixels, viewport heights (pages), or to a specific element. Essential for interacting with lazy-loaded content or infinite scrolling.

**📡 Network Inspection (v0.3.0)**
* `get_network_logs`: Retrieve intercepted network requests (REST/HTTP) and WebSocket frames.
* **Advanced Filtering**: Filter logs by URL, resource type, WebSocket direction, or payload content.
* **Payload Inspection**: Access full request/response headers, REST response bodies, and WebSocket frames.
* **Context Optimized**: Optional "summary mode" to avoid flooding the LLM context window.

**🪵 Console & Errors (v0.6.0)**
* `get_console_logs`: Retrieve console logs from the browser. This includes console.log/warn/error calls, exceptions, and network errors. Crucial for troubleshooting page scripts and errors. Includes optional log level filtering and a `clear` flag to manage state efficiently.

**⚡ Performance & Profiling (v0.7.1)**
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

**🧪 Stability & Reliability**
* **Extensive Unit Testing**: Comprehensive test suite ensuring the reliability of event processing and tool deserialization, particularly in the `debugger` domain.
* **Side-Effect Free Tests**: All unit tests are designed to run in isolation, without launching real Chrome instances or modifying the filesystem.
* **Internal Refactoring**: Decoupled core logic through traits and dependency injection to ensure long-term maintainability.

---

## ⚙️ Configuration

By default, the MCP Server attempts to find the Chrome executable in standard OS-specific locations (e.g., `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` on macOS, or `chrome` in your system `PATH` on Windows).

**Arguments:**
* `--local`: Restricts navigation to local addresses only (`localhost`, `127.0.0.1`, `192.168.x.x`, or `*.local`). Highly recommended for security.
* `--headless`: Runs Chrome in headless mode (no GUI). Essential for Docker or server environments.
* `--host`: Specifies the target host for the Chrome instance (default: `127.0.0.1`). Use `host.docker.internal` to connect to a host machine from a container.
* `--port`: Specifies the remote debugging port (default: `9222`).
* `--enable-automation`: Enables the "controlled by automated software" infobar.

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
      "args": ["run", "-i", "--rm", "chrome-debug-mcp:v1.0.3", "--headless"]
    },
    "chrome-docker-hybrid": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "--net=host",
        "chrome-debug-mcp:v1.0.3",
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