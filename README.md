<p align="center">
  <img src="logo.png" alt="Sootie logo" width="128">
</p>

<h1 align="center">Sootie</h1>

<p align="center">
  A Rust computer-use runtime that gives AI agents one portable MCP tool surface
  for desktop apps, browser pages, screenshots, recipes, and vision grounding.
</p>

<p align="center">
  <a href="docs/api/mcp-tools-reference.md">Tools</a>
  ·
  <a href="docs/guides/browser-cdp.md">CDP guide</a>
  ·
  <a href="docs/api/recipe-schema.md">Recipes</a>
  ·
  <a href="docs/development/runtime-smoke-runbook.md">Runtime checks</a>
</p>

## Why Sootie

Sootie is built for agents that need to operate real computers through a stable
tool contract instead of one-off UI scripts. It runs as an MCP server over
stdio, exposes `sootie_*` tools, and keeps the public argument and response
shapes portable across macOS, Linux, and Windows.

The current runtime resolves targets through the strongest available signal:
browser CDP for DOM-backed pages, the native desktop backend for app and window
state, and vision grounding as the final fallback. A `vision-only` mode is also
available when you want to test or force the visual grounding path directly.

## What It Can Do

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

Sootie exposes 29 MCP tools.

| Area | Tools |
| --- | --- |
| Orientation and perception | `sootie_context`, `sootie_state`, `sootie_find`, `sootie_read`, `sootie_inspect`, `sootie_element_at`, `sootie_screenshot`, `sootie_parse_screen`, `sootie_ground`, `sootie_annotate` |
| Actions | `sootie_click`, `sootie_type`, `sootie_press`, `sootie_hotkey`, `sootie_scroll`, `sootie_hover`, `sootie_long_press`, `sootie_drag`, `sootie_focus`, `sootie_window`, `sootie_wait` |
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

## Recipes

Recipes are JSON documents that can be saved, listed, inspected, deleted, and
run through the MCP tool surface. A recipe can encode action steps, wait steps,
parameter substitution, and legacy recorded step shapes.

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
