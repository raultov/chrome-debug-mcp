# chrome-debug-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org)

**chrome-debug-mcp** is an asynchronous Rust-based **Model Context Protocol (MCP)** server that allows AI agents and Large Language Models to natively control, automate, and debug Chromium-based browsers via the **Chrome DevTools Protocol (CDP)**.

Using `cdp-lite` underneath, this MCP server directly hooks into the browser avoiding heavy abstractions, enabling live-debugging sessions directly from your editor or chat-interface. Starting from v0.2.0, it can also manage the Chrome process lifecycle automatically.

<a href="https://glama.ai/mcp/servers/raultov/chrome-debug-mcp">
  <img width="380" height="200" src="https://glama.ai/mcp/servers/raultov/chrome-debug-mcp/badge" alt="chrome-debug-mcp MCP server" />
</a>

---

## ✨ Features (v0.6.0)

This server natively implements a suite of tools categorized by CDP domains and native process management:

**🚀 Chrome Instance Management (v0.2.4)**
* **Auto-Launch**: Automatically detects if Chrome is running on port 9222. If not, it spawns a new instance with the required flags.
* `restart_chrome`: Restarts the managed Chrome instance.
* `stop_chrome`: Shuts down the managed Chrome instance gracefully (SIGTERM/SIGINT with fallback to SIGKILL).
* **Robust Lifecycle**: Fixed issues with dangling Chrome processes and patched preferences for cleaner restarts.

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
Configure your AI client (like Claude Desktop, Zed, Cursor, or Gemini CLI) to execute the installed binary.

Example configuration for Claude Desktop (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "chrome-debug-mcp": {
      "command": "chrome-debug-mcp",
      "args": [],
      "env": {}
    }
  }
}
```
*Note: If you downloaded the binary manually, replace `"chrome-debug-mcp"` with the absolute path to the executable.*

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