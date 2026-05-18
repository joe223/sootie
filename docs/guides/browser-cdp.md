# Browser Automation with CDP

Sootie can automate browser pages through CDP when a browser is launched with a
remote debugging port. This is the preferred path for DOM-backed browser
targets because it is more precise than desktop accessibility alone.

CDP does not add a separate MCP tool family. The same `sootie_*` tools are used
for desktop and browser targets; Sootie chooses CDP internally when the target
is browser DOM content and a debugging endpoint is available.

## When CDP Is Used

Sootie uses CDP when one of these is true:

- `SOOTIE_CDP_PORT` points to a reachable debugging port.
- `SOOTIE_CDP_WS_URL` points directly to a page WebSocket endpoint.
- A supported browser process is already running with
  `--remote-debugging-port`.

If CDP is unavailable, Sootie falls back to the platform desktop backend and
screenshots.

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

## Why Keep Desktop Fallbacks

CDP only covers browser content. Sootie still needs platform backends for:

- launching and focusing apps;
- non-browser desktop apps;
- windows, screenshots, and coordinates outside the page;
- browsers that were not started with remote debugging.

For browser workflows, the best path is CDP first, platform fallback second.
For non-browser workflows, Sootie uses the native desktop backend directly.
