# Browser Automation with CDP

Sootie can automate browser pages through CDP when a browser is launched with a
remote debugging port. This is the preferred path for DOM-backed browser
targets because it is more precise than desktop accessibility alone.

Sootie exposes CDP on two layers:

- portable `sootie_*` tools still use CDP internally when a browser DOM target
  is available, then fall back to platform and vision paths;
- browser-native `sootie_browser_*` tools use CDP directly and do not fall back
  to desktop automation.

## When CDP Is Used

Sootie uses CDP when one of these is true:

- `SOOTIE_CDP_PORT` points to a reachable debugging port.
- `SOOTIE_CDP_WS_URL` points directly to a page WebSocket endpoint.
- A supported browser process is already running with
  `--remote-debugging-port`.

If CDP is unavailable, portable `sootie_*` calls fall back to the platform
desktop backend and screenshots. Browser-native `sootie_browser_*` calls return
`BROWSER_NOT_CONNECTED` or another browser-specific error instead.

## Browser Setup

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

## How It Works

CDP exposes a debugging WebSocket for the active page. Sootie uses it to:

- read page text and form values with `Runtime.evaluate`;
- collect visible DOM elements and their viewport bounds;
- click, type, hover, long-press, drag, press keys, and scroll by dispatching
  browser input events;
- capture page screenshots with `Page.captureScreenshot`;
- include page URLs in context snapshots.

This means a browser button can be found by DOM id, class, text, role-like
labels, or computed name even when the desktop accessibility tree is sparse.

## Browser-Native Tools

Browser-native tools are meant for browser-first agents and web automation:

| Tool | Purpose |
| --- | --- |
| `sootie_browser_launch` | Launch Chrome, Edge, or Chromium with a managed CDP endpoint and return a `launch_id`. |
| `sootie_browser_connect` | Connect to a CDP endpoint and return pages. |
| `sootie_browser_pages` | List current pages/tabs. |
| `sootie_browser_select_page` | Set the default page for later browser calls. |
| `sootie_browser_open` | Open or navigate a page. |
| `sootie_browser_observe` | Return page state, visible text, browser elements, and optional screenshots. |
| `sootie_browser_find` | Find browser elements by ref, selector, role/name/text, DOM id/class, or query. |
| `sootie_browser_click` | Click a browser element through CDP. |
| `sootie_browser_type` | Type into a browser element through CDP. |
| `sootie_browser_press` | Dispatch a browser key event. |
| `sootie_browser_scroll` | Scroll the page or a target element. |
| `sootie_browser_wait` | Wait for page lifecycle, URL/title/text, or element conditions. |
| `sootie_browser_extract` | Extract page content as text, markdown, HTML, or JSON. |
| `sootie_browser_screenshot` | Capture a page screenshot through CDP. |
| `sootie_browser_back` | Navigate back. |
| `sootie_browser_forward` | Navigate forward. |
| `sootie_browser_reload` | Reload the page. |
| `sootie_browser_close_page` | Close a page by id. |
| `sootie_browser_shutdown` | Stop a browser process previously started by `sootie_browser_launch`. |
| `sootie_browser_network` | Inspect performance/resource entries or guarded response bodies. |
| `sootie_browser_console` | Read console entries captured by the page hook. |
| `sootie_browser_storage` | List, read, or mutate localStorage/sessionStorage. |
| `sootie_browser_cookies` | List, read, or mutate cookies. |
| `sootie_browser_downloads` | Configure download behavior with explicit unsafe opt-in. |
| `sootie_browser_upload` | Set files on a file input with explicit unsafe opt-in. |
| `sootie_browser_pdf` | Render the page as PDF. |
| `sootie_cdp_send` | Send a guarded raw CDP command. |
| `sootie_cdp_subscribe` | Collect a bounded batch of CDP events. |

`sootie_browser_observe` and `sootie_browser_find` return compact browser
elements with a stable short-lived `ref` such as `br_17`. The browser element
registry reuses refs for the same element across adjacent calls on the same page
and resolves them back to selectors or coordinates before action dispatch.
Durable recipes should still prefer `role`, `name`, `text`, `dom_id`, or
`selector` because refs expire after navigation, page close, or significant DOM
updates. `observe` accepts `include` flags and `viewport_only` so agents can
request only the browser state they need.

`sootie_browser_extract` can extract the whole page, a top-level `selector` or
`ref`, or a nested `target` object such as `{ "target": { "ref": "br_3" } }`.
Recipes should prefer durable selectors over transient refs.

Sensitive browser operations are gated:

- `sootie_browser_storage` requires `unsafe: true` for every storage action,
  including `list` and `get`.
- `sootie_browser_cookies` requires `unsafe: true` for every cookie action,
  including `list` and `get`.
- `sootie_browser_downloads`, `sootie_browser_upload`, response-body reads, and
  raw CDP calls require `unsafe: true`.
- High-risk raw CDP methods also require `SOOTIE_ENABLE_UNSAFE_RAW_CDP=1`.

Example flow:

```json
{ "name": "sootie_browser_connect", "arguments": { "port": 9222 } }
```

```json
{ "name": "sootie_browser_open", "arguments": { "url": "https://example.com", "new_page": true } }
```

```json
{ "name": "sootie_browser_observe", "arguments": { "mode": "snapshot" } }
```

```json
{ "name": "sootie_browser_click", "arguments": { "ref": "br_3" } }
```

## Why Keep Desktop Fallbacks

CDP only covers browser content. Sootie still needs platform backends for:

- launching and focusing apps;
- non-browser desktop apps;
- windows, screenshots, and coordinates outside the page;
- browsers that were not started with remote debugging.

For portable computer-use workflows, the best path is CDP first, platform
fallback second. For browser-native workflows, use `sootie_browser_*` so failure
states stay browser-specific and predictable. For non-browser workflows, Sootie
uses the native desktop backend directly.
