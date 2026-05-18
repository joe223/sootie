# Real Runtime Checklist

Use this checklist on machines that can run the target desktop session. The
commands are intentionally small and should be run from the Sootie repository
root after building the release binary. For a copy-paste smoke sequence and
evidence template, use [Runtime Smoke Runbook](runtime-smoke-runbook.md).

```bash
cargo build --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --no-run
target/release/sootie doctor
target/release/sootie doctor --check --raw
target/release/sootie tools --raw
```

CDP is the only intentional capability beyond the selected public tool
contract. It should be verified as a backend path for the existing `sootie_*`
tools, not as a separate tool family.

## macOS

Prerequisites:

- Screen Recording permission for the calling terminal or host app.
- Accessibility permission for the calling terminal or host app.
- Automation permission prompts accepted when controlling apps.
- Microphone or audio-recording permission is not required for the current MCP
  desktop automation surface.

Checks:

1. `target/release/sootie doctor --check --raw` exits successfully and `doctor`
   reports `platform: "macos"`, `runtime_ready: true`, and `launch_context`
   with the Sootie executable plus launching parent process.
2. MCP `sootie_context` returns the frontmost app.
3. MCP `sootie_screenshot` returns `image/png`.
4. MCP `sootie_parse_screen`, `sootie_annotate`, `sootie_ground`, and
   `sootie_element_at` agree on at least one visible target.
5. Browser CDP smoke passes against a temporary remote-debugging browser
   profile.

Troubleshooting:

- If `sootie_context` returns `app: "unknown"` and `sootie_find` returns
  `count: 0`, verify the host app that launched `sootie serve` has
  Accessibility and Screen Recording permissions, then fully restart that host
  app.
- If `target/release/sootie doctor --check --raw` reports `app: "unknown"`, inspect
  `launch_context` and confirm the app or terminal that launches the Sootie
  binary is attached to an active Aqua desktop session.
- If `macos_accessibility` fails, treat it as an Accessibility boundary for the
  host app that launched Sootie. Browser URL fallbacks may still require a
  browser Apple Event prompt.
- If `sootie_focus` cannot make an already-running app frontmost, confirm the
  launcher has Accessibility permission and the target app has a normal visible
  window.
- If `sootie_screenshot` returns `could not create image from display`, first
  run `screencapture -x /tmp/sootie-check.png` from the same host session. If
  that command fails too, the issue is the macOS session or Screen Recording
  permission boundary, not the MCP protocol.
- If `runtime_diagnostics` includes `macos_window_server` with no visible
  windows, the Sootie process is not attached to an active Aqua desktop session
  even if other process-listing probes still work. Restart from a GUI terminal
  or permissioned MCP host with an awake display.
- If `screencapture` fails while the screen looks visible, run
  `system_profiler SPDisplaysDataType`. A report that lists only the GPU and no
  active display means the current process session cannot see a capturable
  display.
- If direct MCP stdio smokes pass but an agent cannot call Sootie tools, verify
  the client attachment separately. For Codex CLI, `codex mcp get sootie` must
  show the Sootie server enabled, and the agent session must be restarted so the
  tool namespace is loaded at session initialization.
- If a fresh client session can list Sootie tools but cancels a read-only tool
  before Sootie logs `tools/call`, rebuild the configured release binary and
  verify `tools/list` includes MCP annotations such as `readOnlyHint: true` for
  `sootie_learn_status`.
- If the screen is visible and a Claude Code fresh-session check still cannot
  call a Sootie tool, first run the no-MCP control prompt from
  [Runtime Smoke Runbook](runtime-smoke-runbook.md). If that control prompt
  cannot reach the Claude API, fix Claude Code authentication, network, or
  proxy configuration before debugging Sootie desktop permissions.

## Linux

For the current acceptance scope, Linux must have implementation plus clean
build/link evidence. Full desktop runtime verification is documented below for
Linux machines, but it is not required on this macOS host.

Prerequisites:

- Interactive X11 desktop session.
- `xprop`, `wmctrl`, and `xdotool` installed.
- Python 3 AT-SPI bindings installed, such as `python3-pyatspi` or
  `python3-gi` plus `gir1.2-atspi-2.0`.
- One screenshot utility installed: `gnome-screenshot`, ImageMagick `import`,
  or `scrot`.
- A browser available for CDP checks, or a documented reason CDP is unavailable
  in that environment.

Checks:

1. Build/link gates pass on the Linux machine:
   `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo test --workspace --all-targets --no-run`.
2. `target/release/sootie doctor --check --raw` exits successfully and `doctor`
   reports `platform: "linux"`, `runtime_ready: true`, and `launch_context`
   showing the shell/session used for X11 or desktop access.
3. MCP `sootie_state` lists visible desktop apps.
4. MCP `sootie_context` returns app and window data for the frontmost app.
5. MCP `sootie_screenshot` returns `image/png`.
6. MCP pointer and keyboard actions are tested against a disposable app or
   browser profile.
7. CDP smoke passes with:

```bash
SOOTIE_CDP_PORT=9222 target/release/sootie serve
```

## Windows

For the current acceptance scope, Windows must have implementation plus clean
build/link evidence. Full desktop runtime verification is documented below for
Windows machines, but it is not required on this macOS host.

Prerequisites:

- Interactive desktop session.
- PowerShell available.
- UI Automation access available from the calling shell or host app.
- Windows Forms and System.Drawing assemblies available for keyboard input and
  screenshots.
- At least one visible top-level application window.
- Chrome or Edge available for CDP checks, or a documented reason CDP is
  unavailable in that environment.

Checks:

1. Build/link gates pass on the Windows machine:
   `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo test --workspace --all-targets --no-run`.
2. `target/release/sootie.exe doctor --check --raw` exits successfully and `doctor`
   reports `platform: "windows"`, `runtime_ready: true`, `launch_context`, and
   no failed diagnostics for PowerShell, UI Automation, Forms/Drawing, or
   visible window access.
3. MCP `sootie_state` lists visible desktop apps.
4. MCP `sootie_context` returns app and window data for the frontmost app.
5. MCP `sootie_screenshot` returns `image/png`.
6. MCP pointer and keyboard actions are tested against a disposable app or
   browser profile.
7. CDP smoke passes after launching Chrome or Edge with
   `--remote-debugging-port=9222`.

## Required Gate Before Completion

For the current acceptance scope, do not mark completion until:

- macOS has full runtime evidence: `doctor`, real MCP stdio smokes, full
  29-tool coverage, screenshot artifact, CDP smoke, and client attachment
  evidence.
- Linux has implementation plus build/link evidence. Full Linux desktop
  runtime evidence is optional unless the scope changes to require a Linux
  desktop machine.
- Windows has implementation plus build/link evidence. Full Windows desktop
  runtime evidence is optional unless the scope changes to require a Windows
  desktop machine.

After collecting the per-platform evidence JSON files, run the evidence gate
from the repository root:

```bash
node docs/development/verify-runtime-evidence.mjs \
  --evidence path/to/macos-evidence.json \
  --evidence path/to/linux-evidence.json \
  --evidence path/to/windows-evidence.json \
  --build-only linux \
  --build-only windows
```

The gate must pass before the current platform acceptance objective can be
considered complete. Use `--platform <name>` only for partial audits; a partial
audit is not completion evidence for the full objective.
