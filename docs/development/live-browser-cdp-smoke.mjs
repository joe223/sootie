#!/usr/bin/env node

import fs from "node:fs";
import http from "node:http";
import { spawn } from "node:child_process";

function argValue(args, name, fallback = null) {
  const index = args.indexOf(name);
  if (index === -1) {
    return fallback;
  }
  if (index + 1 >= args.length) {
    throw new Error(`${name} requires a value`);
  }
  return args[index + 1];
}

function usage() {
  console.error(
    [
      "Usage:",
      "  node docs/development/live-browser-cdp-smoke.mjs --cdp-port 9338",
      "",
      "Options:",
      "  --server <path>       Sootie binary. Default: ./target/debug/sootie",
      "  --cdp-port <number>   Existing Chrome remote-debugging HTTP port.",
      "  --timeout-ms <number> Per-request timeout. Default: 12000.",
      "  --verbose             Print each smoke step before it runs.",
    ].join("\n"),
  );
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function decodeToolPayload(response) {
  const result = response.result || {};
  if (result.structuredContent) {
    return result.structuredContent;
  }
  const text = result.content?.find?.((item) => item.type === "text")?.text;
  if (text) {
    try {
      return JSON.parse(text);
    } catch {
      return result;
    }
  }
  return result;
}

function createMcpClient({ serverPath, cdpPort, timeoutMs }) {
  const child = spawn(serverPath, ["serve"], {
    cwd: process.cwd(),
    env: {
      ...process.env,
      SOOTIE_CDP_PORT: String(cdpPort),
      SOOTIE_ENABLE_UNSAFE_RAW_CDP: "1",
    },
    stdio: ["pipe", "pipe", "pipe"],
  });

  let buffer = "";
  let stderr = "";
  let nextId = 1;
  const pending = new Map();

  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString("utf8");
  });

  child.stdout.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    let index;
    while ((index = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line) {
        continue;
      }
      let message;
      try {
        message = JSON.parse(line);
      } catch (error) {
        for (const slot of pending.values()) {
          slot.reject(new Error(`invalid JSON response: ${error.message}: ${line}`));
        }
        pending.clear();
        continue;
      }
      const slot = pending.get(message.id);
      if (slot) {
        pending.delete(message.id);
        slot.resolve(message);
      }
    }
  });

  child.on("exit", (code, signal) => {
    for (const slot of pending.values()) {
      slot.reject(
        new Error(
          `sootie exited ${code ?? signal}: ${stderr.trim().slice(-1000)}`,
        ),
      );
    }
    pending.clear();
  });

  async function request(method, params = {}) {
    const id = `browser-smoke-${nextId++}`;
    const message = { jsonrpc: "2.0", id, method, params };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (pending.delete(id)) {
          reject(
            new Error(
              `timeout waiting for ${method} ${JSON.stringify(params)}; stderr=${stderr
                .trim()
                .slice(-1000)}`,
            ),
          );
        }
      }, timeoutMs);
      timer.unref();
      pending.set(id, {
        resolve: (response) => {
          clearTimeout(timer);
          if (response.error) {
            reject(
              new Error(`${method} JSON-RPC error: ${JSON.stringify(response.error)}`),
            );
            return;
          }
          resolve(response);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      child.stdin.write(`${JSON.stringify(message)}\n`);
    });
  }

  async function call(name, args = {}) {
    const response = await request("tools/call", { name, arguments: args });
    const payload = decodeToolPayload(response);
    if (payload?.success !== true) {
      throw new Error(`${name} failed: ${JSON.stringify(payload).slice(0, 2000)}`);
    }
    return payload;
  }

  async function close() {
    if (!child.killed) {
      child.stdin.end();
      child.kill("SIGTERM");
    }
    await Promise.race([
      new Promise((resolve) => child.once("close", resolve)),
      new Promise((resolve) => {
        setTimeout(() => {
          child.kill("SIGKILL");
          resolve();
        }, 2000).unref();
      }),
    ]);
  }

  return { request, call, close };
}

async function startFixtureServer() {
  const pixel = Buffer.from(
    "R0lGODlhAQABAPAAAP///wAAACH5BAAAAAAALAAAAAABAAEAAAICRAEAOw==",
    "base64",
  );
  const server = http.createServer((request, response) => {
    if (request.url === "/pixel.gif") {
      response.writeHead(200, {
        "Content-Type": "image/gif",
        "Cache-Control": "no-store",
      });
      response.end(pixel);
      return;
    }
    if (request.url === "/api") {
      response.writeHead(200, {
        "Content-Type": "text/plain",
        "Cache-Control": "no-store",
      });
      response.end("api-ok");
      return;
    }
    response.writeHead(200, {
      "Content-Type": "text/html; charset=utf-8",
      "Set-Cookie": "server_cookie=leaf; Path=/; SameSite=Lax",
      "Cache-Control": "no-store",
    });
    response.end(`<!doctype html>
<html>
<head><title>Sootie Browser Smoke</title></head>
<body>
  <main>
    <label for="name">Name</label>
    <input id="name" aria-label="Name" placeholder="Name" />
    <button id="set" aria-label="Set flower" onclick="console.log('clicked', document.querySelector('#name').value); document.cookie='sootie_cookie=petal; Path=/; SameSite=Lax'; localStorage.setItem('clicked','true'); document.querySelector('#result').textContent='Hello ' + document.querySelector('#name').value;">Set flower</button>
    <input id="file" aria-label="Upload file" type="file" />
    <div id="result">waiting</div>
    <img id="pixel" src="/pixel.gif" alt="pixel" />
  </main>
  <script>
    console.log('ready');
    localStorage.setItem('initial','seed');
    fetch('/api').then((r) => r.text()).then((text) => { window.apiText = text; });
  </script>
</body>
</html>`);
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });

  return {
    url: `http://127.0.0.1:${server.address().port}/`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    return;
  }

  const serverPath = argValue(args, "--server", "./target/debug/sootie");
  const cdpPort = Number(argValue(args, "--cdp-port"));
  const timeoutMs = Number(argValue(args, "--timeout-ms", "12000"));
  const verbose = args.includes("--verbose");
  assert(Number.isInteger(cdpPort) && cdpPort > 0, "--cdp-port must be a positive integer");
  assert(Number.isFinite(timeoutMs) && timeoutMs > 0, "--timeout-ms must be positive");

  const step = (name) => {
    if (verbose) {
      console.error(`[live-browser-cdp-smoke] ${name}`);
    }
  };

  const uploadPath = "/tmp/sootie-browser-upload-smoke.txt";
  fs.writeFileSync(uploadPath, "sootie upload smoke\n", "utf8");

  const fixture = await startFixtureServer();
  const client = createMcpClient({ serverPath, cdpPort, timeoutMs });
  const evidence = {};

  try {
    step("initialize");
    await client.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "sootie-live-browser-smoke", version: "0.0.0" },
    });

    step("tools/list");
    const toolsList = await client.request("tools/list", {});
    evidence.tool_count = toolsList.result.tools.length;
    assert(evidence.tool_count === 57, `expected 57 tools, got ${evidence.tool_count}`);

    step("sootie_browser_connect");
    const connect = await client.call("sootie_browser_connect", { port: cdpPort });
    evidence.connected = connect.data.connected;

    step("sootie_browser_open");
    const opened = await client.call("sootie_browser_open", {
      port: cdpPort,
      url: fixture.url,
      new_page: true,
      wait_until: "domcontentloaded",
      timeout_ms: 5000,
    });
    const pageId = opened.data.page_id;
    evidence.page_id = pageId;

    step("sootie_browser_observe");
    const observe = await client.call("sootie_browser_observe", {
      port: cdpPort,
      page_id: pageId,
      mode: "hybrid",
      include: { elements: true, text: true },
      max_elements: 50,
    });
    const stableRefs = observe.data.elements.map((element) => element.ref).filter(Boolean);
    evidence.stable_ref_sample = stableRefs.find((ref) => /^br_\d+$/.test(ref));
    assert(evidence.stable_ref_sample, `observe returned no stable br_* refs: ${stableRefs}`);

    step("sootie_browser_find input");
    const input = await client.call("sootie_browser_find", {
      port: cdpPort,
      page_id: pageId,
      dom_id: "name",
    });
    const inputRef = input.data.elements[0]?.ref;
    assert(/^br_\d+$/.test(inputRef), `input ref was not stable: ${inputRef}`);
    step("sootie_browser_type stable ref");
    await client.call("sootie_browser_type", {
      port: cdpPort,
      page_id: pageId,
      ref: inputRef,
      text: "Ada",
      clear: true,
    });

    step("sootie_browser_console install hook");
    await client.call("sootie_browser_console", {
      port: cdpPort,
      page_id: pageId,
      max_entries: 5,
    });
    step("sootie_browser_find button");
    const button = await client.call("sootie_browser_find", {
      port: cdpPort,
      page_id: pageId,
      dom_id: "set",
    });
    const buttonRef = button.data.elements[0]?.ref;
    assert(/^br_\d+$/.test(buttonRef), `button ref was not stable: ${buttonRef}`);
    step("sootie_browser_click stable ref");
    await client.call("sootie_browser_click", {
      port: cdpPort,
      page_id: pageId,
      ref: buttonRef,
      wait_after: "stable",
    });
    step("sootie_browser_wait textExists");
    await client.call("sootie_browser_wait", {
      port: cdpPort,
      page_id: pageId,
      condition: "textExists",
      value: "Hello Ada",
      timeout_ms: 5000,
    });

    step("sootie_browser_extract");
    const extracted = await client.call("sootie_browser_extract", {
      port: cdpPort,
      page_id: pageId,
      target: { selector: "#result" },
      format: "text",
      max_chars: 100,
    });
    assert(
      String(extracted.data.content).includes("Hello Ada"),
      `extract did not read the result: ${JSON.stringify(extracted.data)}`,
    );

    step("sootie_browser_network");
    const network = await client.call("sootie_browser_network", {
      port: cdpPort,
      page_id: pageId,
      max_entries: 20,
    });
    evidence.network_count = network.data.requests.length;
    assert(
      network.data.requests.some((request) => {
        const url = String(request.url ?? request.name ?? "");
        return url.includes("/pixel.gif") || url.includes("/api");
      }),
      `network did not include fixture resources: ${JSON.stringify(network.data.requests)}`,
    );

    step("sootie_browser_console read");
    const consoleEntries = await client.call("sootie_browser_console", {
      port: cdpPort,
      page_id: pageId,
      max_entries: 20,
    });
    evidence.console_count = consoleEntries.data.entries.length;
    assert(
      consoleEntries.data.entries.some((entry) => {
        const text = JSON.stringify(entry.args ?? entry.text ?? entry.message ?? entry);
        return text.includes("clicked") && text.includes("Ada");
      }),
      `console hook missed click log: ${JSON.stringify(consoleEntries.data.entries)}`,
    );

    step("sootie_browser_storage set");
    await client.call("sootie_browser_storage", {
      port: cdpPort,
      page_id: pageId,
      area: "localStorage",
      action: "set",
      key: "flower",
      value: "red",
      unsafe: true,
    });
    step("sootie_browser_storage get");
    const storage = await client.call("sootie_browser_storage", {
      port: cdpPort,
      page_id: pageId,
      area: "localStorage",
      action: "get",
      key: "flower",
      unsafe: true,
    });
    assert(
      storage.data.storage.value === "red",
      `storage get mismatch: ${JSON.stringify(storage.data)}`,
    );

    step("sootie_browser_cookies list");
    const cookies = await client.call("sootie_browser_cookies", {
      port: cdpPort,
      page_id: pageId,
      action: "list",
      unsafe: true,
    });
    evidence.cookie_names = cookies.data.cookies.cookies.map((cookie) => cookie.name).sort();
    assert(
      evidence.cookie_names.includes("server_cookie") &&
        evidence.cookie_names.includes("sootie_cookie"),
      `cookies missing expected names: ${JSON.stringify(cookies.data.cookies.cookies)}`,
    );

    step("sootie_browser_upload");
    await client.call("sootie_browser_upload", {
      port: cdpPort,
      page_id: pageId,
      selector: "#file",
      file_paths: [uploadPath],
      unsafe: true,
    });

    step("sootie_browser_screenshot");
    const screenshot = await client.call("sootie_browser_screenshot", {
      port: cdpPort,
      page_id: pageId,
      format: "png",
    });
    evidence.screenshot_chars = String(screenshot.data.image || "").length;
    assert(evidence.screenshot_chars > 1000, "screenshot payload is unexpectedly small");

    step("sootie_browser_pdf");
    const pdf = await client.call("sootie_browser_pdf", {
      port: cdpPort,
      page_id: pageId,
      print_background: true,
    });
    evidence.pdf_bytes = pdf.data.byte_length;
    assert(evidence.pdf_bytes > 1000, `pdf too small: ${JSON.stringify(pdf.data).slice(0, 500)}`);

    step("sootie_cdp_send Browser.getVersion");
    const version = await client.call("sootie_cdp_send", {
      port: cdpPort,
      page_id: pageId,
      method: "Browser.getVersion",
      unsafe: true,
    });
    evidence.browser_product = version.data.result.product;

    step("sootie_cdp_subscribe Log");
    const events = await client.call("sootie_cdp_subscribe", {
      port: cdpPort,
      page_id: pageId,
      domain: "Log",
      timeout_ms: 150,
      max_events: 2,
      unsafe: true,
    });
    evidence.subscribed_events = events.data.events.length;

    step("sootie_browser_downloads deny");
    await client.call("sootie_browser_downloads", {
      port: cdpPort,
      page_id: pageId,
      action: "deny",
      unsafe: true,
    });
    step("sootie_browser_reload");
    await client.call("sootie_browser_reload", {
      port: cdpPort,
      page_id: pageId,
      timeout_ms: 5000,
    });
    step("sootie_browser_close_page");
    await client.call("sootie_browser_close_page", { port: cdpPort, page_id: pageId });

    console.log(JSON.stringify({ ok: true, page_url: fixture.url, evidence }, null, 2));
  } finally {
    await client.close();
    await fixture.close();
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exitCode = 1;
});
