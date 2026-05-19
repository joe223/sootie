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
  <a href="#tool-surface">55 MCP tools</a>
  ·
  <a href="#recipes-and-learning">Recipes</a>
  ·
  <a href="#runtime-check">Runtime checks</a>
</p>

Sootie is a Rust MCP runtime that gives any MCP-capable agent one computer-use
contract across macOS, Linux, and Windows. Use it from OpenCode, Claude Code,
Codex, Cursor, VS Code, or your own agent runtime.

The agent keeps calling the same `sootie_*` tools while Sootie chooses the best
execution path underneath: browser DOM through CDP, native OS backends for real
desktop state, and vision grounding when structure runs out.

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

Sootie runs as an MCP server over stdio and exposes `sootie_*` tools with
portable argument and response shapes. Each target is resolved through the
strongest available signal:

1. Browser CDP for DOM-backed pages.
2. Native platform backends for apps, windows, and desktop state.
3. Vision grounding when structural signals are not enough.

A `vision-only` mode is also available when you want to test or force the visual
grounding path directly.

## Install

macOS:

```bash
brew install joe223/sootie/sootie
sootie setup
```

Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/joe223/sootie/apt/sootie.list \
  | sudo tee /etc/apt/sources.list.d/sootie.list >/dev/null
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
outside that range, install a compatible Python first.

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

Sootie exposes 55 MCP tools.

| Area | Tools |
| --- | --- |
| Orientation and perception | `sootie_context`, `sootie_state`, `sootie_find`, `sootie_read`, `sootie_inspect`, `sootie_element_at`, `sootie_screenshot`, `sootie_parse_screen`, `sootie_ground`, `sootie_annotate` |
| Actions | `sootie_click`, `sootie_type`, `sootie_press`, `sootie_hotkey`, `sootie_scroll`, `sootie_hover`, `sootie_long_press`, `sootie_drag`, `sootie_focus`, `sootie_window`, `sootie_wait` |
| Browser-native CDP | `sootie_browser_connect`, `sootie_browser_pages`, `sootie_browser_select_page`, `sootie_browser_open`, `sootie_browser_observe`, `sootie_browser_find`, `sootie_browser_click`, `sootie_browser_type`, `sootie_browser_press`, `sootie_browser_scroll`, `sootie_browser_wait`, `sootie_browser_extract`, `sootie_browser_screenshot`, `sootie_browser_back`, `sootie_browser_forward`, `sootie_browser_reload`, `sootie_browser_close_page`, `sootie_browser_network`, `sootie_browser_console`, `sootie_browser_storage`, `sootie_browser_cookies`, `sootie_browser_downloads`, `sootie_browser_upload`, `sootie_browser_pdf` |
| Guarded raw CDP | `sootie_cdp_send`, `sootie_cdp_subscribe` |
| Recipes and learning | `sootie_recipes`, `sootie_run`, `sootie_recipe_show`, `sootie_recipe_save`, `sootie_recipe_delete`, `sootie_learn_start`, `sootie_learn_stop`, `sootie_learn_status` |

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

CDP is used through the existing `sootie_*` tools. If CDP is unavailable or the
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
`~/.config/sootie.config.toml` when you want `sootie_ground`, `sootie_find`,
`sootie_inspect`, and target-based pointer actions to go directly through the
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
