# MCP Tools Reference

Sootie exposes MCP tools over stdio JSON-RPC. Tool names are Sootie-specific
and use the `sootie_*` prefix. Input and output data shapes are portable across
macOS, Linux, and Windows backends.

## Transport

Clients call tools through `tools/call`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "sootie_context",
    "arguments": {}
  }
}
```

Sootie accepts both Content-Length framed MCP messages and newline-delimited
JSON messages.

## Compatible Input Envelopes

For compatibility with common MCP clients and recorded recipes, Sootie accepts
arguments in these equivalent forms:

```json
{ "name": "sootie_click", "arguments": { "query": "Save" } }
```

```json
{ "name": "sootie_click", "data": { "query": "Save" } }
```

```json
{ "name": "sootie_click", "input": { "query": "Save" } }
```

Nested `data`, `input`, and `params` envelopes are flattened before dispatch,
except `sootie_run.params`, which is preserved as recipe parameter data.

The flattened `arguments` object is checked against the public `tools/list`
field names, required fields, and JSON value types before dispatch. Direct MCP
calls must use the field names and types in the table below. Compatibility
aliases used by older recipes or internal migration helpers, such as nested
`target`, `bounds`, millisecond timing aliases, app-identity wrapper objects,
comma-separated key lists, and recipe JSON objects, are not accepted as public
`tools/call` arguments.

## Common Fields

| Field | Shape | Meaning |
| --- | --- | --- |
| `app` | string | Human app name or platform identity selector. |
| `query` | string | Text selector for visible or DOM-backed elements. |
| `role` | string | Element role selector. |
| `identifier` | string | Platform accessibility identifier selector. |
| `dom_id` | string | Browser DOM id selector when CDP is available. |
| `dom_class` | string | Browser DOM class selector when CDP is available. |
| `x`, `y` | number | Screen coordinates. |

## Response Shape

Every tool result is returned as MCP content plus structured content. The
structured content follows this shape:

```json
{
  "success": true,
  "data": {},
  "context": {},
  "error": null,
  "suggestion": null,
  "report": {
    "tool": "sootie_context",
    "arguments": {},
    "duration_ms": 12,
    "success": true,
    "error": null
  }
}
```

On failure, `success` is `false`, `error` contains the user-facing failure, and
`suggestion` is populated when Sootie can identify a likely recovery path.

Screenshot-producing tools return image data on two paths:

- MCP `content` includes an `image` item with base64 data and `mimeType` for
  clients that can pass image content directly to a multimodal model.
- `structuredContent.data` keeps the base64 `image` field and, when Sootie can
  persist the bytes to the system temp directory, also includes `artifact_path`
  and `artifact_uri` for clients that need to load the captured image by file.

## Tool Annotations

`tools/list` includes MCP tool annotations so clients can distinguish
read-only inspection from mutating desktop actions before dispatch:

- Read-only tools such as `sootie_context`, `sootie_find`,
  `sootie_screenshot`, and `sootie_learn_status` set `readOnlyHint: true`,
  `destructiveHint: false`, and `idempotentHint: true`.
- Desktop actions such as `sootie_click`, `sootie_type`, `sootie_hotkey`,
  `sootie_drag`, `sootie_window`, and `sootie_run` set `readOnlyHint: false`
  and are marked as potentially mutating.
- Local recipe writes use non-read-only annotations even when they do not touch
  the desktop.

## Tool Surface

| Tool | Required fields | Optional fields | Result data |
| --- | --- | --- | --- |
| `sootie_context` | none | `app` | `app`, `window`, `url`, `focused_element`, `interactive_elements` |
| `sootie_state` | none | `app` | `apps`, `app_count` |
| `sootie_find` | none | `app`, `query`, `role`, `identifier`, `dom_id`, `dom_class`, `depth` | `elements`, `count`, `total_matches` |
| `sootie_read` | none | `app`, `query`, `depth` | `content`, `item_count` |
| `sootie_inspect` | `query` | `app`, `role`, `dom_id` | one full element |
| `sootie_element_at` | `x`, `y` | none | one full element |
| `sootie_screenshot` | none | `app`, `full_resolution` | `image`, `mime_type`, `width`, `height`, `window_title`, `window_frame`, `artifact_path`, `artifact_uri` |
| `sootie_click` | none | `app`, `query`, `role`, `dom_id`, `x`, `y`, `button`, `count` | action result plus context |
| `sootie_type` | `text` | `app`, `into`, `dom_id`, `clear` | action result plus context |
| `sootie_press` | `key` | `app`, `modifiers` | action result plus context |
| `sootie_hotkey` | `keys` | `app` | action result plus context |
| `sootie_scroll` | `direction` | `app`, `amount`, `x`, `y` | action result plus context |
| `sootie_hover` | none | `app`, `query`, `role`, `dom_id`, `x`, `y` | action result plus context |
| `sootie_long_press` | none | `app`, `query`, `role`, `dom_id`, `x`, `y`, `duration`, `button` | action result plus context |
| `sootie_drag` | `to_x`, `to_y` | `app`, `from_x`, `from_y`, `query`, `role`, `dom_id`, `duration`, `hold_duration` | action result plus context |
| `sootie_focus` | `app` | `window` | action result plus context |
| `sootie_window` | `action`, `app` | `window`, `x`, `y`, `width`, `height` | action result plus context |
| `sootie_wait` | `condition` | `app`, `value`, `timeout`, `interval` | wait result |
| `sootie_recipes` | none | none | `recipes` |
| `sootie_run` | `recipe` | `params` | recipe step results. See [Recipe Schema](recipe-schema.md). |
| `sootie_recipe_show` | `name` | none | recipe object |
| `sootie_recipe_save` | `recipe_json` | none | saved recipe metadata. See [Recipe Schema](recipe-schema.md). |
| `sootie_recipe_delete` | `name` | none | `deleted` |
| `sootie_parse_screen` | none | `app`, `full_resolution` | screenshot payload plus `elements`, `element_count`, `source` |
| `sootie_ground` | `description` | `app`, `crop_box` | ranked candidates or vision-grounded point |
| `sootie_annotate` | none | `app`, `roles`, `max_labels` | annotated image payload and text index |
| `sootie_learn_start` | none | `task_description` | recording status |
| `sootie_learn_stop` | none | none | recorded actions, apps, urls, duration |
| `sootie_learn_status` | none | none | recording status and action count |

## Wait Conditions

`sootie_wait.condition` accepts these exact values:

- `elementExists`
- `elementGone`
- `titleContains`
- `titleChanged`
- `urlContains`
- `urlChanged`

Pass the match string through `value`. Timeout and interval values are seconds
via `timeout` and `interval`.

## Browser CDP Behavior

When a browser exposes CDP through `SOOTIE_CDP_PORT`, `SOOTIE_CDP_WS_URL`, or a
discoverable local remote-debugging process, browser DOM targets use CDP first.
CDP does not add a separate public tool family; the same `sootie_*` tools route
browser DOM targets through CDP when possible. CDP-backed operations can read
DOM text, enumerate elements, resolve element bounds, dispatch pointer and
keyboard events, scroll the page, and capture page screenshots. If CDP is
unavailable or the target is outside browser content, Sootie falls back to the
platform backend.

## Vision Grounding Behavior

When `sootie_find` or pointer actions cannot resolve a described target through
CDP or the platform accessibility tree, Sootie can call the local vision sidecar
installed by `sootie setup`. Configure it with `SOOTIE_VISION_URL` or
`SOOTIE_VISION_PORT`; by default Sootie probes `http://127.0.0.1:9876`. Set
`SOOTIE_VISION_DISABLED=1` to skip this path. Sootie also reads
`~/.config/sootie.config.toml`; environment variables override file values.

Default target resolution remains platform-first: CDP DOM and the native
accessibility backend resolve targets first, with vision used only as the final
fallback. To test the vision chain in isolation, set the strategy to
`vision-only`:

```toml
[resolution]
strategy = "vision-only"

[vision]
url = "http://127.0.0.1:9876"
enabled = true
confidence_threshold = 0.5
timeout_ms = 60000
sidecar_dir = "/path/to/sootie/vision-sidecar"
model_path = "/path/to/sootie/models/ShowUI-2B"
```

In `vision-only` mode, described target resolution for `sootie_ground`,
`sootie_find`, `sootie_inspect`, target-based pointer actions, and target-based
drag points uses `/ground` directly instead of platform/CDP lookup. Explicit
coordinates still execute as coordinates.

Every successful vision grounding attempt stores an annotated JPG screenshot
under `/tmp/sootie/vision_history/grounding/`. The image overlays the prompt,
returned bounding boxes, prediction values, and numbered labels for multiple
matches on top of the screenshot sent to the sidecar. The companion JSON file
records the request frame, crop box, predictions, and sidecar result.
`sootie_ground` includes `vision_screenshot_path`, `vision_screenshot_uri`,
`vision_screenshot_mime_type`, `vision_metadata_path`, and
`vision_metadata_uri` in `structuredContent.data` when the artifact is written.

The sidecar contract is `POST /ground` with this JSON payload:

```json
{
  "image": "<base64 screenshot>",
  "description": "Send button",
  "screen_w": 1440,
  "screen_h": 900,
  "crop_box": [500, 150, 840, 420]
}
```

The expected response contains either top-level coordinates:

```json
{
  "x": 620,
  "y": 350,
  "confidence": 0.9,
  "method": "full-screen",
  "inference_ms": 1200
}
```

or a `matches` array with normalized or absolute `point` and `bbox` values.
`crop_box` is accepted in screen coordinates; Sootie maps it to the screenshot
frame before sending it to the sidecar and maps returned coordinates back to
screen coordinates. Vision fallback is used by `sootie_ground`, `sootie_find`,
`sootie_click`, `sootie_hover`, and `sootie_long_press`.

Use `sootie setup` to install the bundled Python sidecar dependencies into a
Sootie-managed virtual environment, download the default `showlab/ShowUI-2B`
model snapshot when it is missing, and verify model preload. Use
`sootie sidecar` to run the sidecar on port `9876`. Vision dependency setup
requires Python 3.10-3.13.

## Verification Commands

```bash
target/release/sootie tools --raw
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```
