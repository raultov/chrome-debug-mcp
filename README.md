# chrome-control-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org)

**chrome-control-mcp** is an asynchronous Rust-based **Model Context Protocol (MCP)** server that allows AI agents and Large Language Models to natively control, automate, and debug Chromium-based browsers via the **Chrome DevTools Protocol (CDP)**.

Using `cdp-lite` underneath, this MCP server directly hooks into the browser avoiding heavy abstractions, enabling live-debugging sessions directly from your editor or chat-interface.

---

## ✨ Features (v0.1.0)

This server natively implements a suite of tools categorized by CDP domains:

**🌐 Page & Runtime Control**
* `connect_chrome`: Establish connection to a Chrome CDP remote debugging port (e.g. `127.0.0.1:9222`).
* `navigate`: Navigate the active tab to a specific URL.
* `reload`: Reload the current page.
* `inspect_dom`: Extract the entire HTML payload of the current document.
* `evaluate_js`: Run an arbitrary JavaScript expression globally on the page context.

**🐞 Live Debugging & Execution Control**
* `pause_on_load`: Enables the debugger and triggers a page reload, pausing execution on the very first parsed script statement.
* `search_scripts`: Search across all parsed script contexts for a query to accurately find lines and columns for breakpoints.
* `set_breakpoint`: Set a precise JS breakpoint using `script_id`, `url`, or exact `script_hash`.
* `evaluate_on_call_frame`: Evaluate a JavaScript expression directly inside the *local scope* of the currently paused debugger call frame.
* `step_over`: Step over the next expression line.
* `resume`: Unpause and resume the execution.
* `remove_breakpoint`: Remove a previously set breakpoint.

---

## 🚀 Quick Start

To use this server, you must have an MCP compatible client (like Claude Desktop, Zed, Cursor, etc.). You must configure the client to execute the server binary.

### 1. Launch Chrome in Debugging Mode
Before issuing commands, make sure your browser is listening for remote CDP connections:
```sh
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/remote-profile
```

### 2. Configure MCP Client
You can use the pre-built binaries from the [Releases](https://github.com/raultov/chrome-control-mcp/releases) page, or compile it locally via `cargo build --release`. 

Example of a standard MCP client `config.json`:
```json
{
  "mcpServers": {
    "chrome-control-mcp": {
      "command": "/path/to/bin/chrome-control-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

Then, instruct the LLM (e.g., using `connect_chrome`) to attach to `127.0.0.1:9222`.

---

## 🛠 Compilation (From Source)

Require Rust toolchain installed:

```bash
git clone https://github.com/raultov/chrome-control-mcp
cd chrome-control-mcp
cargo build --release
```

The resulting binary will be located in `target/release/chrome-control-mcp`.

---

## 📖 Why this MCP Server?

Other integration servers like Puppeteer/Playwright wrappers are high-level, heavy, and typically fail at exposing **real, interactive step-by-step debuggers**. This MCP server uses raw CDP messages mapping them 1:1 to LLM tools, which allows intelligent agents to *literally* step over JS, read local scope variables natively, search inside V8 compiler contexts, and understand exactly why a script is crashing.

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for more details.
