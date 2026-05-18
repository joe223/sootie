#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const repoRoot = process.cwd();

function usage() {
  console.error(
    [
      "Usage:",
      "  node docs/development/run-jsonl-mcp-smoke.mjs --output path/to/responses.jsonl",
      "",
      "Options:",
      "  --template <path>          Request JSONL file.",
      "  --output <path>            Response JSONL file to write.",
      "  --server <path>            Server binary. Default: target/release/sootie",
      "  --arg <value>              Server arg. Repeatable. Default: serve",
      "  --env <KEY=VALUE>          Environment variable for the server. Repeatable.",
      "  --timeout-ms <number>      Smoke timeout. Default: 120000",
      "  --framed                   Send Content-Length framed MCP messages.",
      "  --allow-placeholders       Allow <placeholder> strings in the template.",
      "  --no-initialize            Do not prepend an MCP initialize request.",
    ].join("\n"),
  );
}

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

function repeatedArgValues(args, name, fallback) {
  const values = [];
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === name) {
      if (index + 1 >= args.length) {
        throw new Error(`${name} requires a value`);
      }
      values.push(args[index + 1]);
      index += 1;
    }
  }
  return values.length > 0 ? values : fallback;
}

function parseEnvValues(values) {
  const env = {};
  for (const value of values) {
    const separator = value.indexOf("=");
    if (separator <= 0) {
      throw new Error(`--env values must use KEY=VALUE: ${value}`);
    }
    env[value.slice(0, separator)] = value.slice(separator + 1);
  }
  return env;
}

function resolveRepoPath(input) {
  return path.isAbsolute(input) ? input : path.join(repoRoot, input);
}

function readTemplate(file, allowPlaceholders) {
  const text = fs.readFileSync(file, "utf8");
  if (!allowPlaceholders && /<[^>\r\n]+>/.test(text)) {
    throw new Error(
      `${file} still contains placeholders; replace them or pass --allow-placeholders`,
    );
  }
  return text
    .split(/\r?\n/)
    .map((line, index) => ({ line: line.trim(), lineNumber: index + 1 }))
    .filter((entry) => entry.line.length > 0)
    .map((entry) => {
      try {
        return { ...entry, request: JSON.parse(entry.line) };
      } catch (error) {
        throw new Error(`${file}:${entry.lineNumber}: ${error.message}`);
      }
    });
}

function validateTemplateRequests(entries, initialize) {
  const ids = new Set();
  if (initialize) {
    ids.add("smoke-init");
  }
  return entries.map((entry) => {
    const { request } = entry;
    if (request.id === undefined || request.id === null) {
      throw new Error(`template:${entry.lineNumber}: request id is required`);
    }
    const id = String(request.id);
    if (ids.has(id)) {
      throw new Error(`template:${entry.lineNumber}: duplicate request id ${id}`);
    }
    ids.add(id);
    return JSON.stringify(request);
  });
}

function countJsonLines() {
  let pending = "";
  let count = 0;
  return {
    push(chunk) {
      const text = pending + chunk;
      const parts = text.split(/\r?\n/);
      pending = parts.pop() || "";
      count += parts.filter((line) => line.trim().length > 0).length;
    },
    finish() {
      if (pending.trim().length > 0) {
        count += 1;
      }
      return count;
    },
  };
}

function contentLengthFrame(message) {
  const body = Buffer.from(message, "utf8");
  return `Content-Length: ${body.length}\r\n\r\n${message}`;
}

function framedJsonlWriter(output) {
  let buffer = Buffer.alloc(0);
  let count = 0;
  return {
    push(chunk) {
      buffer = Buffer.concat([buffer, chunk]);
      while (buffer.length > 0) {
        const separator = buffer.indexOf("\r\n\r\n");
        if (separator === -1) {
          return;
        }
        const header = buffer.slice(0, separator).toString("utf8");
        const match = header.match(/Content-Length:\s*(\d+)/i);
        if (!match) {
          throw new Error(`missing Content-Length header: ${header}`);
        }
        const bodyStart = separator + 4;
        const bodyLength = Number(match[1]);
        const frameEnd = bodyStart + bodyLength;
        if (buffer.length < frameEnd) {
          return;
        }
        const body = buffer.slice(bodyStart, frameEnd).toString("utf8");
        output.write(`${JSON.stringify(JSON.parse(body))}\n`);
        count += 1;
        buffer = buffer.slice(frameEnd);
      }
    },
    finish() {
      if (buffer.toString("utf8").trim().length > 0) {
        throw new Error("incomplete Content-Length frame in server output");
      }
      return count;
    },
  };
}

async function runSmoke({
  server,
  serverArgs,
  templateRequests,
  outputFile,
  timeoutMs,
  initialize,
  framed,
  extraEnv,
}) {
  fs.mkdirSync(path.dirname(outputFile), { recursive: true });
  const output = fs.createWriteStream(outputFile);
  const child = spawn(server, serverArgs, {
    cwd: repoRoot,
    env: { ...process.env, ...extraEnv },
    stdio: ["pipe", "pipe", "pipe"],
  });

  const counter = framed ? framedJsonlWriter(output) : countJsonLines();
  let stderr = "";
  let timedOut = false;

  const timer = setTimeout(() => {
    timedOut = true;
    child.kill("SIGTERM");
    setTimeout(() => child.kill("SIGKILL"), 2000).unref();
  }, timeoutMs);

  child.stdout.on("data", (chunk) => {
    if (framed) {
      counter.push(chunk);
    } else {
      output.write(chunk);
      counter.push(chunk.toString("utf8"));
    }
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString("utf8");
  });

  const requests = initialize
    ? [
        JSON.stringify({
          jsonrpc: "2.0",
          id: "smoke-init",
          method: "initialize",
          params: {
            protocolVersion: "2024-11-05",
            capabilities: {},
            clientInfo: { name: "sootie-jsonl-smoke", version: "0.0.0" },
          },
        }),
        ...templateRequests,
      ]
    : templateRequests;

  for (const request of requests) {
    child.stdin.write(framed ? contentLengthFrame(request) : `${request}\n`);
  }
  child.stdin.end();

  const result = await new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code, signal) => resolve({ code, signal }));
  });
  clearTimeout(timer);
  output.end();

  const responseCount = counter.finish();
  return {
    transport: framed ? "content-length" : "line-json",
    env_keys: Object.keys(extraEnv).sort(),
    request_count: requests.length,
    response_count: responseCount,
    output: path.relative(repoRoot, outputFile),
    exit_code: result.code,
    signal: result.signal,
    timed_out: timedOut,
    stderr_tail: stderr.trim().slice(-2000),
  };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    return;
  }

  const templateFile = resolveRepoPath(
    argValue(args, "--template", "docs/development/full-tool-smoke-requests.jsonl"),
  );
  const outputArg = argValue(args, "--output");
  if (!outputArg) {
    usage();
    throw new Error("--output is required");
  }

  const timeoutMs = Number(argValue(args, "--timeout-ms", "120000"));
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive number");
  }

  const server = argValue(args, "--server", "target/release/sootie");
  const serverArgs = repeatedArgValues(args, "--arg", ["serve"]);
  const extraEnv = parseEnvValues(repeatedArgValues(args, "--env", []));
  const templateEntries = readTemplate(
    templateFile,
    args.includes("--allow-placeholders"),
  );
  const templateRequests = validateTemplateRequests(
    templateEntries,
    !args.includes("--no-initialize"),
  );
  const framed = args.includes("--framed");

  const result = await runSmoke({
    server,
    serverArgs,
    templateRequests,
    outputFile: resolveRepoPath(outputArg),
    timeoutMs,
    initialize: !args.includes("--no-initialize"),
    framed,
    extraEnv,
  });

  console.log(JSON.stringify(result, null, 2));
  if (
    result.timed_out ||
    result.exit_code !== 0 ||
    result.response_count !== result.request_count
  ) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
