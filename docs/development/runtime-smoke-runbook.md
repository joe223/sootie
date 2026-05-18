# Runtime Smoke Runbook

Use this runbook to collect real runtime evidence before claiming a platform is
ready. Run commands from the Sootie repository root after building the release
binary.

```bash
cargo build --release
target/release/sootie doctor --check --raw
target/release/sootie tools --raw
```

Save the raw JSON output with the platform name and date. A platform smoke is
not complete until it includes doctor output, one perception call, one
screenshot call, one action call, and one browser CDP call or an explicit
reason CDP is unavailable.

The minimum smoke above is only a triage gate. A platform is not fully verified
until the full 29-tool coverage suite below has a captured response for every
public `sootie_*` tool, or a documented platform/environment reason for any
unavailable CDP-backed browser step.

Keep the raw JSON-RPC responses. The evidence should show both the MCP transport
shape and the Sootie structured result fields (`success`, `data`, `error`,
`report.tool`, and `report.duration_ms`).

## Build and Link Gates

Before collecting desktop runtime evidence, verify the target platform can
compile and link the Sootie crates, including test harnesses:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --no-run
```

For cross-target checks from a non-native host, also run the target-specific
compile and lint gates:

```bash
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace --all-targets --target x86_64-unknown-linux-gnu --no-run
cargo test --workspace --all-targets --target x86_64-pc-windows-msvc --no-run
```

The `clippy --target` commands verify Rust type checking for those backends.
The `cargo test --no-run --target ...` commands additionally require a working
target linker and target system libraries:

- Linux needs a GNU-compatible linker plus the target Linux sysroot/libraries.
  Using `rust-lld` alone is not enough if `libc`, `libpthread`, `libdl`, and
  other target libraries are missing.
- Windows MSVC needs `link.exe` or a compatible linker plus Windows SDK import
  libraries such as `kernel32.lib` and `user32.lib`. Using `rust-lld` alone is
  not enough without those SDK libraries.

If the target test-harness build reaches the link step and fails only because
the linker, sysroot, or SDK is missing, record that as a host toolchain gap.
Do not count it as a Sootie backend failure, but do not mark the target's
test-harness build verified until it passes on a machine with the required
target toolchain.

For the current acceptance scope, macOS requires the full runtime smoke in this
runbook. Linux and Windows require implementation plus build/link evidence; run
their full runtime smoke only when a target desktop machine is available or the
scope changes to require it.

## MCP Stdio Smoke

Start the server:

```bash
target/release/sootie serve
```

Send these JSON-RPC requests over the same stdio connection:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"sootie_context","arguments":{}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sootie_find","arguments":{"query":""}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"sootie_screenshot","arguments":{"full_resolution":false}}}
```

Expected evidence:

- `initialize` returns server info with platform.
- `sootie_context` returns `success: true` and the current app or window.
- `sootie_find` returns `success: true` and a numeric `count`.
- `sootie_screenshot` returns `success: true`, `mime_type: "image/png"`,
  and non-zero image dimensions.

## Framed MCP Smoke

At least one smoke should use standard `Content-Length` MCP framing, not only
newline-delimited JSON. This validates the path used by typical MCP clients.

Required framed calls:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"runtime-smoke","version":"0.0.0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sootie_learn_status","arguments":{}}}
```

Expected evidence:

- `initialize` returns `serverInfo.name: "sootie"`.
- `tools/list` returns exactly 29 tools.
- `tools/list` marks `sootie_learn_status` with `readOnlyHint: true`.
- `sootie_learn_status` returns `success: true`.

## MCP Client Attachment Smoke

Direct stdio success proves the Sootie server can speak MCP, but it does not
prove a long-running agent client has loaded that server into the current
session. Capture client attachment evidence separately.

For Codex CLI, check the configured server:

```bash
codex mcp get sootie
```

Expected evidence:

- The client reports `enabled: true`, `transport: stdio`, command
  `target/release/sootie`, and argument `serve`.
- A newly started agent session exposes the Sootie MCP tool namespace.
- A newly started agent session can call `sootie_learn_status` and receives
  `success: true`.
- `target/release/sootie serve` stays alive while the client session is active.

If direct line-json and `Content-Length` smokes pass but the agent session has
no Sootie tool namespace, restart or recreate the client session. Treat that as
a client attachment or session hot-reload issue, not a screen, permission, or
Sootie runtime failure.

When diagnosing an attachment failure, inspect the Sootie log file. A healthy
request path records `MCP request received`, `Sootie tool call completed`, and
`MCP stdin closed`. A log with only the startup line means the client process
did not send a request before the server exited.

If the client lists tools but reports that a read-only call was cancelled before
Sootie logs `tools/call`, check that the configured binary is the current
release build and that `tools/list` includes MCP annotations. Missing
annotations can make non-interactive clients route the call through an approval
path that cannot complete.

### Client Matrix

Use this matrix to keep client-specific evidence separate from direct stdio
evidence. Do not mark a client as verified unless the client can both list the
server and dispatch at least one read-only Sootie tool call in a fresh session.

| Client | Local check | Required evidence | Current status |
| --- | --- | --- | --- |
| Codex CLI | `codex mcp get sootie` | `enabled: true`, `transport: stdio`, command points to `target/release/sootie`, args include `serve`, and a fresh session can call `sootie_learn_status`. | Verified on 2026-05-16 for configured command/args and fresh-session read-only call. |
| Claude Code | `claude mcp get sootie` | Server named `sootie` is configured, health check starts the stdio server, and a fresh Claude Code session can call `sootie_learn_status`. | Project config health check verified on 2026-05-16 with `.mcp.json`; fresh-session tool call still needs capture. A no-MCP `--bare` control prompt failed with `FailedToOpenSocket`, so fix Claude Code auth/network/proxy before treating the missing tool call as Sootie evidence. |
| Cursor | Cursor MCP settings plus a fresh agent session | Settings point to `target/release/sootie serve`, tools are visible in the session, and `sootie_learn_status` returns `success: true`. | External client gate. |
| VS Code | VS Code MCP settings plus a fresh agent session | Settings point to `target/release/sootie serve`, tools are visible in the session, and `sootie_learn_status` returns `success: true`. | External client gate. |

Adding or changing user-level MCP client configuration is a separate
side-effect. Record the exact config change and raw client output if that gate
is executed.

For Claude Code, a bounded non-interactive fresh-session check can be run with:

```bash
claude -p "Call the MCP tool mcp__sootie__sootie_learn_status exactly once and report whether the returned structured content has success true. Do not call any other tool." \
  --mcp-config .mcp.json \
  --strict-mcp-config \
  --allowedTools mcp__sootie__sootie_learn_status \
  --permission-mode dontAsk \
  --output-format json \
  --max-budget-usd 0.02 \
  --no-session-persistence
```

Only mark the Claude Code fresh-session gate as verified when the command
finishes and the output shows a Sootie tool call result with `success: true`.
If it hangs before producing output, treat that as an external client session,
auth, or network startup issue and do not count it as Sootie evidence.

To separate that client startup path from Sootie, run a no-MCP control prompt:

```bash
claude --bare \
  --strict-mcp-config \
  --mcp-config '{"mcpServers":{}}' \
  --tools '' \
  --no-session-persistence \
  --max-budget-usd 0.02 \
  -p 'Return exactly: ok'
```

If this control prompt cannot reach the Claude API, or if it hangs without
printing the expected `ok`, fix the Claude Code authentication, network, proxy,
or session-startup path first. Sootie cannot receive a `tools/call` until the
client session itself can start. If the control prompt works but the Sootie
prompt hangs, compare the Sootie log: `initialize` plus `tools/list` only means
the client listed tools but never dispatched a tool; `tools/call` plus `Sootie
tool call failed` means the request reached Sootie and the failure is in the
Sootie runtime path.

## Recipe and Learning Smoke

Run this with a temporary home/data directory so the test does not write into a
user's normal recipe store.

Required calls:

```json
{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"sootie_recipe_save","arguments":{"recipe_json":"{\"schema_version\":3,\"name\":\"smoke-delay\",\"params\":[],\"steps\":[{\"action\":\"wait\",\"timeout\":0,\"params\":null}]}"}}}
{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"sootie_recipe_show","arguments":{"name":"smoke-delay"}}}
{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"sootie_recipes","arguments":{}}}
{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"sootie_run","arguments":{"recipe":"smoke-delay"}}}
{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"sootie_learn_start","arguments":{"task_description":"runtime smoke"}}}
{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"sootie_window","arguments":{"app":"<visible disposable app>","action":"list"}}}
{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"sootie_learn_status","arguments":{}}}
{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"sootie_learn_stop","arguments":{}}}
{"jsonrpc":"2.0","id":28,"method":"tools/call","params":{"name":"sootie_recipe_delete","arguments":{"name":"smoke-delay"}}}
```

Expected evidence:

- `sootie_run` returns `success: true` and `steps_completed: 1`.
- `sootie_learn_status` reports `recording: true` and `action_count: 1`.
- `sootie_learn_stop` returns one recorded action.
- `sootie_recipe_delete` returns `deleted: true`.

## Full 29-Tool Coverage Smoke

Use this gate before marking any platform runtime complete. The smaller smokes
above prove that the runtime is reachable; this table proves that every public
tool has a real response on the target platform.

Run against a disposable visible app/window and a disposable browser profile
when CDP is available. Save the raw JSON-RPC response for every row.

Use [full-tool-smoke-requests.jsonl](full-tool-smoke-requests.jsonl) as the
copy-paste request template. Replace placeholder values such as
`<disposable app>`, `<visible target text>`, and `<visible window title>` before
running it, and execute it only against disposable windows because the action
tools intentionally mutate the target app.
The template starts learning mode before the `sootie_window(action=list)` call
so `sootie_learn_status` and `sootie_learn_stop` can prove action recording.

The Node.js helper scripts in this section are optional evidence helpers, not
Sootie runtime dependencies. If Node is unavailable on a target machine, send
the JSON-RPC requests manually or with another MCP client, then check the same
requirements: all 29 public tools present, no unknown tools, `success: true`,
`report.success: true`, and numeric `report.duration_ms` for each tool result.

Suggested placeholder replacements:

| Platform | `<disposable app>` | `<visible target text>` | `<visible window title>` |
| --- | --- | --- | --- |
| macOS | `Calculator` or a throwaway `TextEdit` document | A visible button, menu item, or text field label in that app | The exact window title shown by `sootie_context` or `sootie_window(action=list)` |
| Linux | A visible X11 app name from `wmctrl -l` or `sootie_state` | A visible control or text string inside that app | The title returned by `xdotool getactivewindow getwindowname` or `sootie_window(action=list)` |
| Windows | `notepad` or another throwaway app with a visible window | A visible menu/control string or document text inside that app | The title returned by `sootie_context` or `sootie_window(action=list)` |

Use the same target app for action, wait, and learning checks. If a chosen app
does not expose accessible text for `find/read/inspect`, switch to a browser or
editor window with visible text rather than weakening the expected result.

Before running it, verify the template still matches the compiled tool contract:

```bash
node docs/development/verify-public-tool-contract.mjs
node docs/development/verify-full-tool-smoke.mjs
```

To collect the full runtime artifact bundle on a target machine, use:

```bash
node docs/development/collect-runtime-evidence.mjs \
  --platform macos \
  --disposable-app Calculator \
  --visible-target-text "Calculator" \
  --visible-window-title "Calculator" \
  --run-build-gates \
  --client-configured \
  --client-fresh-session-tool-call
```

Replace the platform and visible target values for Linux or Windows. The
collector runs the cargo build/lint gates, `doctor --check`, a full line-json
MCP smoke, a framed MCP smoke, extracts the screenshot artifact, writes a
dedicated Sootie runtime log, and prints the exact
`verify-runtime-evidence.mjs` command for the generated evidence file. If you
already captured build gates separately, use `--build-gates-passed` or
individual `--build-gate NAME=pass` overrides instead of `--run-build-gates`.
This is useful when the target host runs cargo checks outside a launcher or
sandbox that blocks local TCP mock servers used by the CDP tests; keep those
separate cargo logs next to the generated evidence and pass them as
`--build-artifact NAME=path`. The runtime verifier requires each passing build
gate to have a non-empty log artifact containing `exit_status=0`.
Only pass `--client-configured` and `--client-fresh-session-tool-call` after
those client gates have actually been run and captured separately; otherwise
leave them unset so the final evidence verifier stays red.

After capturing JSONL responses, verify the evidence file:

```bash
node docs/development/verify-full-tool-smoke.mjs --responses path/to/raw-responses.jsonl
```

The response verifier allows repeated tools so a single evidence file can
include additional CDP-specific calls. It still fails on missing public tools,
unknown tool names, failed tool results, missing `report.success: true`, or
missing numeric `report.duration_ms` unless `--allow-failures` is used for
triage. When a response id matches a request id from the template, the verifier
also requires `report.tool` to match that request's `params.name`; this catches
request/response ordering or capture mistakes where the tool set is complete
but individual rows are paired with the wrong request. The verifier also fails
when a template request id has no response or a response id appears more than
once, even if the set of reported tools is complete.

To send a JSONL request file through `target/release/sootie serve` and capture
raw responses, use:

```bash
node docs/development/run-jsonl-mcp-smoke.mjs \
  --template docs/development/full-tool-smoke-requests.jsonl \
  --output path/to/raw-responses.jsonl
```

Use `--framed` on at least one run to send standard `Content-Length` MCP
frames while still writing normalized JSONL responses that the verifier can
read:

```bash
node docs/development/run-jsonl-mcp-smoke.mjs \
  --framed \
  --env SOOTIE_CDP_PORT=9222 \
  --template docs/development/full-tool-smoke-requests.jsonl \
  --output path/to/framed-responses.jsonl
```

Use repeated `--env KEY=VALUE` flags for browser-backend checks, for example
`--env SOOTIE_CDP_PORT=9222` or `--env SOOTIE_CDP_WS_URL=ws://...`.

The runner refuses templates that still contain `<placeholder>` values unless
`--allow-placeholders` is passed, so replace the disposable app, target text,
and window title values before using it as completion evidence.
It also rejects templates with missing request ids, duplicate request ids, or a
request id that conflicts with the prepended `smoke-init` initialize request.
The runner exits non-zero when the captured response count differs from the
request count, even if the server process itself exits successfully.

After each target machine has a complete runtime evidence JSON file, verify the
platform set:

```bash
node docs/development/verify-runtime-evidence.mjs \
  --evidence path/to/macos-evidence.json \
  --evidence path/to/linux-evidence.json \
  --evidence path/to/windows-evidence.json
```

That verifier reads the compiled public tool count from `TOOL_NAMES` and fails
when any required platform is missing, duplicated, blocked by runtime blockers,
missing build/client/smoke pass markers, reporting `unknown` or `loginwindow`
as the runtime app, missing raw artifact files, or attaching a raw JSON-RPC log
that does not contain successful reports for every public tool. It also parses
the saved `doctor` JSON artifact and cross-checks platform, runtime readiness,
context app/window, blockers, and screenshot dimensions against the evidence
summary. The raw JSON-RPC log must also contain responses for every request id
from `full-tool-smoke-requests.jsonl`, with each response report's tool matching
the template request's `params.name`. The screenshot artifact must be a PNG
whose dimensions match the runtime summary, and the Sootie runtime log artifact
must be non-empty. A one-platform partial audit can be run with `--platform
macos`, `--platform linux`, or `--platform windows`, but partial audits do not
satisfy the full completion gate.

| Group | Tools | Required evidence |
| --- | --- | --- |
| Desktop perception | `sootie_context`, `sootie_state`, `sootie_find`, `sootie_read`, `sootie_inspect`, `sootie_element_at`, `sootie_screenshot` | Each tool returns `success: true`; state/context include app identity; read/find/inspect/element-at resolve a visible or DOM-backed target; screenshot returns PNG dimensions. |
| Desktop actions | `sootie_focus`, `sootie_window`, `sootie_hover`, `sootie_click`, `sootie_type`, `sootie_press`, `sootie_hotkey`, `sootie_scroll`, `sootie_long_press`, `sootie_drag`, `sootie_wait` | Each action returns `success: true` or, for waits, `matched: true`; reports include the platform method used; actions are aimed at a disposable app/window. |
| Recipes | `sootie_recipe_save`, `sootie_recipe_show`, `sootie_recipes`, `sootie_run`, `sootie_recipe_delete` | A temporary recipe is saved, shown, listed, run with one completed step, then deleted. |
| Visual grounding | `sootie_parse_screen`, `sootie_ground`, `sootie_annotate` | Parse returns screenshot plus element count; ground returns candidates and a best point for a visible target; annotate returns an annotated image and index. |
| Learning | `sootie_learn_start`, `sootie_learn_status`, `sootie_learn_stop` | Start enables recording; one successful action is recorded; status reports `recording: true`; stop returns the recorded action list. |
| Browser CDP path | DOM-targeted `sootie_find`, `sootie_read`, `sootie_click`, `sootie_type`, `sootie_screenshot`, and one of `sootie_hover`, `sootie_scroll`, `sootie_long_press`, or `sootie_drag` | Browser DOM targets route through CDP methods when remote debugging is enabled. If CDP is unavailable on the target platform, capture the explicit environment reason and do not count CDP as verified. |

Completion criteria for this suite:

- Every public tool listed by `target/release/sootie tools --raw` appears in at least
  one captured row above.
- Every captured response has `report.tool` matching the requested tool.
- Failures are allowed only when the row is explicitly marked as an environment
  blocker, and those blockers keep the platform status unverified.

## macOS

Prerequisites:

- The terminal or app launching Sootie has Accessibility permission.
- The terminal or app launching Sootie has Screen Recording permission.
- Browser Apple Event prompts for browser URL fallback, if used, have been
  accepted for the target browser.
- Microphone or audio-recording permission is not required for the current MCP
  desktop automation surface.

Checks:

```bash
target/release/sootie doctor --check --raw
screencapture -x /tmp/sootie-smoke.png
```

If `doctor --check --raw` fails, inspect `launch_context` and `runtime_diagnostics`.
The current macOS readiness probes are `macos_accessibility` and
`macos_window_server`; failures usually mean the launching app lacks
Accessibility/Screen Recording permission or is not attached to an active Aqua
desktop session.

If `runtime_diagnostics` reports `macos_window_server` with no visible windows,
the process can run but is not attached to an active Aqua desktop session. Move
the smoke to a GUI terminal or permissioned MCP host with an awake display
before treating the result as a Sootie runtime failure.

If `sootie_focus` cannot make an already-running app frontmost, first confirm
the launcher has Accessibility permission and the target app has a normal
visible window. Browser-specific URL reads may still require a browser Apple
Event prompt.

For a low-risk native action smoke, click a visible disposable window title bar
or empty area by coordinate. The expected action result should include
`method: "coregraphics"` so it does not depend on `System Events click at`.

## Linux

Prerequisites:

- Interactive X11 desktop.
- `xprop`, `wmctrl`, and `xdotool` installed.
- Python 3 AT-SPI bindings installed, such as `python3-pyatspi` or
  `python3-gi` plus `gir1.2-atspi-2.0`.
- At least one screenshot utility installed: `gnome-screenshot`, `import`, or
  `scrot`.

Checks:

```bash
target/release/sootie doctor --check --raw
xprop -root _NET_ACTIVE_WINDOW
wmctrl -l
xdotool getactivewindow getwindowname
python3 -c 'import pyatspi'
```

Run the MCP stdio smoke against a visible disposable app or browser window.
Minimum Linux desktop evidence should include these Sootie calls:

```json
{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"sootie_context","arguments":{}}}
{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"sootie_screenshot","arguments":{"full_resolution":false}}}
{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"sootie_window","arguments":{"action":"list"}}}
{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"sootie_hover","arguments":{"x":100,"y":100}}}
{"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"sootie_click","arguments":{"x":100,"y":100}}}
{"jsonrpc":"2.0","id":35,"method":"tools/call","params":{"name":"sootie_wait","arguments":{"condition":"titleContains","value":"<visible window title>","timeout":1}}}
```

Expected evidence:

- `sootie_context` returns a real app or window, not `unknown`.
- `sootie_screenshot` returns a PNG payload with non-zero dimensions.
- `sootie_window(action=list)` returns at least one window.
- Coordinate `hover` and `click` return `success: true`.
- `sootie_wait` returns `matched: true` for the visible window title.

## Windows

Prerequisites:

- Interactive desktop session.
- PowerShell available.
- UI Automation accessible from the launching terminal or host app.
- Windows Forms and System.Drawing assemblies available for keyboard input and
  screenshots.
- At least one visible top-level application window.

Checks:

```bash
target/release/sootie.exe doctor --check --raw
powershell -NoProfile -Command "Get-Process | Select-Object -First 5"
powershell -NoProfile -Command "Add-Type -AssemblyName UIAutomationClient; [System.Windows.Automation.AutomationElement]::RootElement | Out-Null; 'ok'"
powershell -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; 'ok'"
powershell -NoProfile -Command "$window = Get-Process | Where-Object { $_.MainWindowTitle -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1; if (-not $window) { exit 2 }; $window.ProcessName"
```

Run the MCP stdio smoke against a visible disposable app or browser window.
Minimum Windows desktop evidence should include these Sootie calls:

```json
{"jsonrpc":"2.0","id":40,"method":"tools/call","params":{"name":"sootie_focus","arguments":{"app":"notepad"}}}
{"jsonrpc":"2.0","id":41,"method":"tools/call","params":{"name":"sootie_context","arguments":{"app":"notepad"}}}
{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"sootie_screenshot","arguments":{"full_resolution":false}}}
{"jsonrpc":"2.0","id":43,"method":"tools/call","params":{"name":"sootie_window","arguments":{"app":"notepad","action":"list"}}}
{"jsonrpc":"2.0","id":44,"method":"tools/call","params":{"name":"sootie_type","arguments":{"app":"notepad","text":"sootie smoke"}}}
{"jsonrpc":"2.0","id":45,"method":"tools/call","params":{"name":"sootie_wait","arguments":{"app":"notepad","condition":"titleContains","value":"Notepad","timeout":1}}}
```

Expected evidence:

- `sootie_focus` selects an already-running Notepad instance.
- `sootie_context(app=notepad)` returns the Notepad app/window identity.
- `sootie_screenshot` returns a PNG payload with non-zero dimensions.
- `sootie_window(app=notepad, action=list)` returns at least one Notepad
  window.
- `sootie_type` returns `success: true` and the typed byte count.
- `sootie_wait` returns `matched: true` for the Notepad title.

## Browser CDP

Launch a disposable browser profile with remote debugging:

```bash
SOOTIE_CDP_PORT=9222 target/release/sootie serve
```

Then run MCP calls against a simple page:

```json
{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"sootie_context","arguments":{}}}
{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"sootie_read","arguments":{}}}
{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"sootie_find","arguments":{"dom_id":"go"}}}
{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"sootie_click","arguments":{"dom_id":"go"}}}
{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"sootie_type","arguments":{"text":"hello","dom_id":"name"}}}
{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"sootie_screenshot","arguments":{}}}
```

Expected evidence:

- DOM-backed calls return `success: true`.
- Action reports use CDP methods for browser DOM targets.
- Screenshot dimensions match the browser page capture.

## Evidence Template

For macOS runtime evidence, collect the full bundle:

```bash
node docs/development/collect-runtime-evidence.mjs \
  --platform macos \
  --disposable-app Calculator \
  --visible-target-text Calculator \
  --visible-window-title Calculator \
  --run-build-gates \
  --client-configured \
  --client-fresh-session-tool-call
```

For Linux or Windows build-only acceptance evidence, collect or attach build
logs and skip desktop runtime smokes:

```bash
node docs/development/collect-runtime-evidence.mjs \
  --platform linux \
  --build-only \
  --build-gates-passed \
  --build-artifact cargo_build_workspace=path/to/cargo_build_workspace.log \
  --build-artifact cargo_test_workspace=path/to/cargo_test_workspace.log \
  --build-artifact cargo_clippy_workspace=path/to/cargo_clippy_workspace.log \
  --build-artifact target_build=path/to/target_build.log \
  --build-artifact cargo_test_no_run=path/to/cargo_test_no_run.log \
  --build-artifact target_clippy=path/to/target_clippy.log \
  --build-artifact target_test_no_run=path/to/target_test_no_run.log
```

When target gates need a target-specific linker or SDK environment, keep that
environment scoped to the target gates:

```bash
node docs/development/collect-runtime-evidence.mjs \
  --platform linux \
  --build-only \
  --run-build-gates \
  --target-triple x86_64-unknown-linux-musl \
  --target-env 'RUSTFLAGS=-C linker=rust-lld'
```

```json
{
  "platform": "macos",
  "date": "2026-05-16",
  "build": {
    "cargo_build_workspace": "pass",
    "cargo_test_workspace": "pass",
    "cargo_clippy_workspace": "pass",
    "target_build": "pass",
    "cargo_test_no_run": "pass",
    "target_clippy": "pass",
    "target_test_no_run": "pass"
  },
  "build_artifacts": {
    "cargo_build_workspace": "path/to/cargo_build_workspace.log",
    "cargo_test_workspace": "path/to/cargo_test_workspace.log",
    "cargo_clippy_workspace": "path/to/cargo_clippy_workspace.log",
    "target_build": "path/to/target_build.log",
    "cargo_test_no_run": "path/to/cargo_test_no_run.log",
    "target_clippy": "path/to/target_clippy.log",
    "target_test_no_run": "path/to/target_test_no_run.log"
  },
  "runtime": {
    "doctor_ready": true,
    "runtime_blockers": [],
    "context_app": "Calculator",
    "context_window": "Calculator",
    "screenshot_size": {
      "width": 0,
      "height": 0
    }
  },
  "mcp_stdio": {
    "line_json": "pass",
    "content_length": "pass",
    "tool_count": 29
  },
  "smokes": {
    "perception": "pass",
    "screenshot": "pass",
    "action": "pass",
    "recipe": "pass",
    "learning": "pass",
    "cdp": "pass"
  },
  "client_attachment": {
    "client": "Codex CLI",
    "configured": true,
    "fresh_session_tool_call": "pass"
  },
  "artifacts": {
    "doctor_json": "path/to/doctor.json",
    "raw_json_rpc_log": "path/to/raw-responses.jsonl",
    "framed_json_rpc_log": "path/to/framed-responses.jsonl",
    "sootie_runtime_log": "path/to/sootie.log",
    "screenshot": "path/to/screenshot.png"
  },
  "notes": ""
}
```

Build-only evidence may omit the runtime, MCP, smoke, client, and artifact
sections:

```json
{
  "platform": "linux",
  "verification_mode": "build-only",
  "build": {
    "cargo_build_workspace": "pass",
    "cargo_test_workspace": "pass",
    "cargo_clippy_workspace": "pass",
    "target_build": "pass",
    "cargo_test_no_run": "pass",
    "target_clippy": "pass",
    "target_test_no_run": "pass"
  },
  "build_artifacts": {
    "cargo_build_workspace": "path/to/cargo_build_workspace.log",
    "cargo_test_workspace": "path/to/cargo_test_workspace.log",
    "cargo_clippy_workspace": "path/to/cargo_clippy_workspace.log",
    "target_build": "path/to/target_build.log",
    "cargo_test_no_run": "path/to/cargo_test_no_run.log",
    "target_clippy": "path/to/target_clippy.log",
    "target_test_no_run": "path/to/target_test_no_run.log"
  },
  "notes": "Desktop runtime smokes are outside this platform acceptance scope."
}
```

The evidence JSON is intentionally summary-level. Keep the raw build logs,
`doctor` JSON, line-json JSON-RPC log, framed JSON-RPC log, Sootie runtime log,
and screenshot paths in `build_artifacts` and `artifacts`. Relative artifact
paths are resolved from the evidence JSON file's directory. The runtime
evidence gate checks that referenced files exist, that passing build logs contain
`exit_status=0`, that runtime-mode `doctor` agrees with the runtime summary, that the framed
log includes the initialize response, `tools/list` with 29 tools and
`sootie_learn_status.readOnlyHint: true`, plus a successful
`sootie_learn_status` report, and that the raw JSON-RPC log includes
successful tool reports for the full public surface with request/response ids
paired against
`full-tool-smoke-requests.jsonl`. It also reads the screenshot PNG dimensions
and requires the runtime log to be non-empty for runtime-mode evidence.
Build-only platforms validated with `--build-only <platform>` intentionally
skip desktop runtime artifacts and smokes. `verify-full-tool-smoke.mjs
--responses` remains useful when you want a standalone view of the same
template-pairing diagnostics.
