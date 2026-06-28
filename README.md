<p align="center">
  <img src="logo.png" alt="Sootie logo" width="128">
</p>

<h1 align="center">Sootie</h1>

<p align="center">
  Cross-platform computer-use for agents that need the whole desktop, not just one browser tab.
</p>

<p align="center">
  <a href="#quick-start">Quick start</a>
  ·
  <a href="#tool-surface">58 MCP tools</a>
  ·
  <a href="#recipes-and-learning">Recipes</a>
  ·
  <a href="#runtime-check">Runtime checks</a>
</p>

Sootie is a Rust MCP runtime that gives any MCP-capable agent one computer-use
contract across macOS, Linux, and Windows. Use it from OpenCode, Claude Code,
Codex, Cursor, VS Code, or your own agent runtime.

The agent keeps calling the same short tools, such as `find`, `click`, and
`browser_open`, while Sootie chooses the best execution path underneath:
browser DOM through CDP, native OS backends for real desktop state, and vision
grounding when structure runs out.

Teach it a workflow once. Save it as a JSON recipe. Run it again from any
agent.

```bash
sootie setup
sootie serve
```

## Watch Sootie Work

<!-- Demo GIF placeholder:
     Replace this block with the Safari + Excalidraw flower + recipe-recording GIF.
     Suggested asset path: docs/assets/sootie-excalidraw-flower-recipe.gif
-->

<p align="center">
  <em>Demo GIF placeholder: Safari + Excalidraw draws a colorful flower, then
  records the workflow as a reusable Sootie recipe.</em>
</p>

## What Makes Sootie Different

Agent frameworks move fast. Desktop automation APIs do not. Sootie makes that
boundary stable.

- Agent-neutral: any MCP-capable client can call the same Sootie tools.
- Platform-neutral: macOS, Linux, and Windows share the same public MCP
  contract while backend-specific mechanics stay below it.
- Signal-aware: browser CDP first, native platform state second, vision
  grounding last.
- Workflow-aware: learning mode records successful desktop actions and recipes
  replay them later.
- Evidence-first: `sootie doctor`, structured tool reports, and full-suite smoke
  docs make runtime readiness inspectable instead of assumed.

## What Agents Can Do

- Inspect the current desktop: apps, windows, URLs, focused elements, visible
  text, screenshots, and interactive elements.
- Act on apps and pages: click, type, press keys, hotkeys, scroll, hover,
  long-press, drag, focus windows, and manage window geometry.
- Use CDP for browser content when Chrome or Edge exposes a remote debugging
  endpoint, without adding a separate browser-only tool family.
- Fall back to vision grounding for described targets, including annotated JPG
  history under `/tmp/sootie/vision_history/grounding/`.
- Save and run JSON recipes, and record successful actions through learning
  mode.
- Report runtime readiness with `sootie doctor` before an MCP client depends on
  the desktop session.

## How It Works

Sootie runs as an MCP server over stdio and exposes short tool names such as
`context`, `click`, and `browser_open` with portable argument and response
shapes. Older `sootie_*` tool names remain accepted as compatibility aliases
for direct JSON-RPC callers and saved recipes. Each target is resolved through
the strongest available signal:

1. Browser CDP for DOM-backed pages.
2. Native platform backends for apps, windows, and desktop state.
3. Vision grounding when structural signals are not enough.

A `vision-only` mode is also available when you want to test or force the visual
grounding path directly.

## Install

Sootie currently publishes package-manager installs for macOS and Linux amd64.
Windows users install from source while the package-manager path is being
finalized.

| Platform | Install path | Notes |
| --- | --- | --- |
| macOS arm64/x64 | Homebrew | Requires a GUI session plus Accessibility and Screen Recording permissions for desktop actions. |
| Linux amd64 | apt | Requires an interactive X11 desktop for desktop actions. The apt package currently targets amd64. |
| Linux arm64 | Cargo source install | No public apt package yet. |
| Windows | Cargo source install | No public package-manager path yet. |

macOS:

```bash
brew install joe223/sootie/sootie
sootie setup
```

Linux:

```bash
sudo install -d -m 0755 /usr/share/keyrings
curl -fsSL https://raw.githubusercontent.com/joe223/sootie/apt/sootie-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/sootie-archive-keyring.gpg >/dev/null
sudo chmod 0644 /usr/share/keyrings/sootie-archive-keyring.gpg
curl -fsSL https://raw.githubusercontent.com/joe223/sootie/apt/sootie.sources \
  | sudo tee /etc/apt/sources.list.d/sootie.sources >/dev/null
sudo apt-get update
sudo apt-get install sootie
sootie setup
```

Windows:

The Windows package-manager path is not finalized yet. Until it is published,
install from source with Cargo:

```powershell
git clone https://github.com/joe223/sootie.git
cd sootie
cargo install --locked --path crates/sootie-cli
sootie setup
```

From an existing checkout on any platform, the development install path is:

```bash
cargo install --locked --path crates/sootie-cli
```

## Install With An Agent

If you want another coding agent to install Sootie on your computer, copy this
prompt into that agent. It tells the agent to choose the best install path for
your OS, fall back to source when needed, and verify the result before stopping.

```text
Install Sootie on this computer and verify that it works.

Rules:
- Detect the operating system and CPU architecture first.
- Prefer the official install path for this platform:
  - macOS arm64/x64: Homebrew, using `brew install joe223/sootie/sootie`
  - Linux amd64: the apt repository documented in the Sootie README
  - Linux arm64 or Windows: install from source with Rust/Cargo
- If the package-manager path is unavailable or fails, clone or update
  https://github.com/joe223/sootie and run
  `cargo install --locked --path crates/sootie-cli`.
- Do not overwrite unrelated user files. Ask before destructive changes,
  uninstalling existing software, or changing global MCP client settings.
- Run `sootie setup`. If vision dependencies or the model download are too
  large, blocked, or unnecessary for browser/desktop-only use, run
  `sootie setup --skip-sidecar` and report that limitation.
- Verify the installation with:
  - `sootie --version`
  - `sootie doctor --check`
  - `sootie tools --raw`
- Confirm that `sootie tools --raw` returns 58 tools and includes
  `browser_open`.
- Configure my MCP client to run `sootie serve` only if I explicitly ask you to
  configure that client.
- Report the install method, binary path, version, verification results, and any
  remaining manual permission steps, such as macOS Accessibility or Screen
  Recording.
```

## Quick Start

Create the user config:

```bash
sootie setup
```

This writes `~/.config/sootie.config.toml`, installs the bundled vision sidecar,
creates the managed Python environment, downloads the default ShowUI-2B model
when it is missing, and verifies that the sidecar can preload the model. Setup
prints progress while it works. A successful setup means the next `sootie serve`
and `sootie sidecar` runs are expected to work: Sootie verifies the desktop
runtime, MCP initialization, tool listing, sidecar startup, and model preload
before returning success.

Vision setup needs a Python 3.10-3.13 interpreter. If your default `python3` is
outside that range, install a compatible Python first. The first setup run also
needs network access to install Python packages and download the ShowUI model,
plus enough disk and memory to preload that model. If you only need browser CDP
or native desktop structure and do not need vision grounding yet, use
`sootie setup --skip-sidecar` and run full setup later.

CLI commands print a readable summary by default. Add `--raw` when a script
needs the original JSON payload, for example `sootie setup --raw`.

Check whether the current desktop session is usable:

```bash
sootie doctor --check
```

Then configure your MCP client to start Sootie:

```json
{
  "mcpServers": {
    "sootie": {
      "type": "stdio",
      "command": "sootie",
      "args": ["serve"]
    }
  }
}
```

For local development without installing the binary, run:

```bash
cargo run -p sootie-cli -- serve
```

## Runtime Check

Before connecting an agent, check whether the current desktop session is usable:

```bash
sootie doctor
sootie doctor --check
sootie tools
```

`sootie doctor` prints a readable readiness summary. `sootie doctor --check`
exits non-zero when the current session is not ready, which makes it suitable
for scripts and smoke runs. Use `sootie doctor --raw` or
`sootie doctor --check --raw` for the full diagnostic JSON. `sootie tools`
prints a compact tool list; use `sootie tools --raw` for the MCP tool schema.

Default serve logs are written under the platform data directory. On macOS this
is:

```text
~/Library/Application Support/sootie/logs/YYYY-MM-DD-HH-MM-SS.log
```

## Tool Surface

Sootie exposes 58 MCP tools.

| Area | Tools |
| --- | --- |
| Orientation and perception | `context`, `state`, `find`, `read`, `inspect`, `element_at`, `screenshot`, `parse_screen`, `ground`, `annotate` |
| Actions | `click`, `type`, `press`, `hotkey`, `scroll`, `hover`, `long_press`, `drag`, `focus`, `window`, `wait` |
| Browser-native CDP | `browser_launch`, `browser_connect`, `browser_pages`, `browser_select_page`, `browser_open`, `browser_observe`, `browser_viewport`, `browser_find`, `browser_click`, `browser_type`, `browser_press`, `browser_scroll`, `browser_wait`, `browser_extract`, `browser_screenshot`, `browser_back`, `browser_forward`, `browser_reload`, `browser_close_page`, `browser_shutdown`, `browser_network`, `browser_console`, `browser_storage`, `browser_cookies`, `browser_downloads`, `browser_upload`, `browser_pdf` |
| Guarded raw CDP | `cdp_send`, `cdp_subscribe` |
| Recipes and learning | `recipes`, `run`, `recipe_show`, `recipe_save`, `recipe_delete`, `learn_start`, `learn_stop`, `learn_status` |

Every tool returns MCP content plus structured content with `success`, `data`,
`context`, `error`, `suggestion`, and a `report` that includes duration and
tool-call status. `tools/list` includes MCP annotations so clients can
distinguish read-only inspection from mutating desktop actions.

See [MCP Tools Reference](docs/api/mcp-tools-reference.md) for accepted fields,
input envelopes, response shapes, and compatibility behavior.

## Browser Automation

Sootie uses CDP internally when a supported browser exposes a debugging
endpoint:

```bash
SOOTIE_CDP_PORT=9222 sootie serve
```

For browser-only work, `browser_launch` starts a managed headless browser
by default so pages, screenshots, and extraction do not interrupt the user's
visible desktop. Pass `mode: "normal"` or `headless: false` when the user needs
to see or manually help with the browser.

macOS Chrome example:

```bash
/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
  --remote-debugging-port=9222 \
  --user-data-dir=/tmp/sootie-chrome-profile
```

Linux Chrome example:

```bash
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/sootie-chrome-profile
```

On Windows, launch Chrome or Edge with `--remote-debugging-port=9222`, then run
Sootie with `SOOTIE_CDP_PORT=9222`.

CDP is used through the existing portable tools. If CDP is unavailable or the
target is outside browser content, Sootie falls back to the native desktop
backend and screenshots. See [Browser Automation with CDP](docs/guides/browser-cdp.md).

## Vision Grounding

By default, Sootie tries CDP and the platform backend first, then uses vision as
the final target-resolution fallback. `sootie setup` writes the default sidecar
URL and model path into `~/.config/sootie.config.toml`; environment variables
can override the sidecar URL:

```bash
SOOTIE_VISION_URL=http://127.0.0.1:9876 sootie serve
```

Default config shape:

```toml
[resolution]
strategy = "platform-first"

[vision]
url = "http://127.0.0.1:9876"
enabled = true
confidence_threshold = 0.5
timeout_ms = 60000
sidecar_dir = "/path/to/sootie/vision-sidecar"
model_path = "/path/to/sootie/models/ShowUI-2B"
```

The Rust MCP server talks to a local HTTP sidecar that implements `POST /ground`.
`sootie setup` installs that sidecar, installs the Python dependencies listed in
the bundled `requirements.txt` into a Sootie-managed virtual environment,
downloads `showlab/ShowUI-2B` into Sootie's data directory when missing, and
checks that the model can be preloaded. The first setup may take a while because
the model download is large and requires network access. Start the sidecar
before using vision-grounded targets:

```bash
sootie sidecar
```

Use `sootie sidecar --preload` when you want startup to load the model before
the first grounding request.

If you do not run a vision sidecar, CDP and native desktop automation still work.
Disable vision with `SOOTIE_VISION_DISABLED=1` or set `enabled = false` in the
config. Set `resolution.strategy = "vision-only"` in
`~/.config/sootie.config.toml` when you want `ground`, `find`,
`inspect`, and target-based pointer actions to go directly through the
vision grounding path.

Successful grounding calls write annotated JPG screenshots and JSON metadata to:

```text
/tmp/sootie/vision_history/grounding/
```

The JPG overlays the prompt, returned bounding boxes, prediction values, and
numbered labels.

## Platform Backends

| Platform | Current backend surface |
| --- | --- |
| macOS | AppKit, Accessibility, CoreGraphics, browser Apple Events where needed, and `screencapture`. Grant Accessibility and Screen Recording permissions to the app or terminal that launches Sootie. |
| Linux | X11-oriented helpers such as `xprop`, `wmctrl`, `xdotool`, AT-SPI bindings, and common screenshot utilities when installed. |
| Windows | PowerShell, User32, UI Automation, Windows Forms, and System.Drawing from an interactive desktop session. |

The public MCP contract stays portable while the Rust backend chooses the
native mechanism available on the current host.

## Recipes and Learning

Recipes are JSON documents that can be saved, listed, inspected, deleted, and
run through the MCP tool surface. A recipe can encode action steps, wait steps,
parameter substitution, and legacy recorded step shapes.

Learning mode records successful actions so an agent can turn a real desktop
workflow into a reusable recipe.

See [Recipe Schema](docs/api/recipe-schema.md) for the full format.

## Verification

Run the local gates before trusting a binary:

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

For runtime evidence, use:

- [Real Runtime Checklist](docs/development/real-runtime-checklist.md)
- [Runtime Smoke Runbook](docs/development/runtime-smoke-runbook.md)
- [Verification Matrix](docs/development/verification-matrix.md)

The runtime checks are intentionally separate from compile-time checks: a
successful MCP handshake or build does not prove that the active desktop
session can actually click, type, see screenshots, or ground visual targets.
