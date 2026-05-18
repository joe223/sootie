#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const repoRoot = process.cwd();
const defaultTemplate = "docs/development/full-tool-smoke-requests.jsonl";
const requiredBuildGates = [
  "cargo_build_workspace",
  "cargo_test_workspace",
  "cargo_clippy_workspace",
  "target_build",
  "cargo_test_no_run",
  "target_clippy",
  "target_test_no_run",
];

function usage() {
  console.error(
    [
      "Usage:",
      "  node docs/development/collect-runtime-evidence.mjs --disposable-app <app> --visible-target-text <text> --visible-window-title <title>",
      "",
      "Options:",
      "  --output-dir <path>              Evidence artifact directory. Default: OS temp dir.",
      "  --platform <name>                Platform label. Default: current Node platform.",
      "  --server <path>                  Server binary. Default: target/release/sootie.",
      "  --template <path>                Full smoke template JSONL.",
      "  --timeout-ms <number>            MCP smoke timeout. Default: 120000.",
      "  --env <KEY=VALUE>                Environment variable for the server. Repeatable.",
      "  --run-build-gates                Run cargo build/test/lint gates and record logs.",
      "  --target-triple <triple>          Target triple for target build/clippy/test-no-run gates.",
      "  --target-env <KEY=VALUE>          Environment variable for target build/clippy/test-no-run gates. Repeatable.",
      "  --build-gates-passed             Mark required build gates pass after running them separately.",
      "  --build-gate <NAME=pass|fail>     Override one build gate status. Repeatable.",
      "  --build-artifact <NAME=path>      Build gate log path for separately run gates. Repeatable.",
      "  --client-configured              Mark MCP client configuration as verified.",
      "  --client-fresh-session-tool-call  Mark fresh client Sootie tool call as verified.",
      "  --build-only                     Collect build/link evidence without desktop runtime smokes.",
      "  --continue-on-doctor-failure      Write doctor output even when runtime is blocked.",
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

function repeatedArgValues(args, name) {
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
  return values;
}

function parseKeyValue(value, flagName) {
  const separator = value.indexOf("=");
  if (separator <= 0) {
    throw new Error(`${flagName} must use NAME=value: ${value}`);
  }
  return [value.slice(0, separator), value.slice(separator + 1)];
}

function parseEnv(values) {
  const env = {};
  for (const value of values) {
    const [key, envValue] = parseKeyValue(value, "--env");
    env[key] = envValue;
  }
  return env;
}

function resolveRepoPath(input) {
  return path.isAbsolute(input) ? input : path.join(repoRoot, input);
}

function currentPlatform() {
  if (process.platform === "darwin") {
    return "macos";
  }
  if (process.platform === "win32") {
    return "windows";
  }
  if (process.platform === "linux") {
    return "linux";
  }
  return process.platform;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env, ...(options.env || {}) },
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) {
    throw result.error;
  }
  if (!options.allowFailure && result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed with ${result.status}\n${result.stderr || result.stdout}`,
    );
  }
  return result;
}

function writeCommandLog(file, command, args, result, env = {}) {
  const envPrefix = Object.entries(env)
    .map(([key, value]) => `${key}=${JSON.stringify(value)}`)
    .join(" ");
  const payload = [
    `$ ${envPrefix ? `${envPrefix} ` : ""}${command} ${args.join(" ")}`,
    "",
    `exit_status=${result.status}`,
    "",
    "## stdout",
    result.stdout || "",
    "",
    "## stderr",
    result.stderr || "",
  ].join("\n");
  fs.writeFileSync(file, payload);
}

function readJsonFromStdout(result, label) {
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`${label} did not print JSON: ${error.message}`);
  }
}

function replacementMap(args) {
  const disposableApp = argValue(args, "--disposable-app");
  const visibleText = argValue(args, "--visible-target-text");
  const visibleWindowTitle = argValue(args, "--visible-window-title");
  const missing = [];
  if (!disposableApp) {
    missing.push("--disposable-app");
  }
  if (!visibleText) {
    missing.push("--visible-target-text");
  }
  if (!visibleWindowTitle) {
    missing.push("--visible-window-title");
  }
  if (missing.length > 0) {
    throw new Error(`missing required smoke values: ${missing.join(", ")}`);
  }
  return new Map([
    ["<disposable app>", disposableApp],
    ["<visible target text>", visibleText],
    ["<visible window title>", visibleWindowTitle],
  ]);
}

function writeResolvedTemplate(templateFile, outputFile, replacements) {
  let text = fs.readFileSync(templateFile, "utf8");
  for (const [placeholder, value] of replacements) {
    text = text.split(placeholder).join(value);
  }
  const remaining = text.match(/<[^>\r\n]+>/g);
  if (remaining) {
    throw new Error(
      `unresolved template placeholders: ${[...new Set(remaining)].join(", ")}`,
    );
  }
  fs.writeFileSync(outputFile, text);
}

function parseJsonLines(file) {
  return fs
    .readFileSync(file, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function responseReport(response) {
  const structured = response.result?.structuredContent || response.result;
  const report = structured?.report;
  return typeof report?.tool === "string" ? { structured, report } : null;
}

function extractScreenshot(rawLogFile, screenshotFile) {
  for (const response of parseJsonLines(rawLogFile)) {
    const item = responseReport(response);
    if (item?.report.tool !== "sootie_screenshot") {
      continue;
    }
    const data = item.structured?.data;
    const image = data?.image;
    const mimeType = data?.mime_type;
    if (typeof image !== "string" || mimeType !== "image/png") {
      continue;
    }
    fs.writeFileSync(screenshotFile, Buffer.from(image, "base64"));
    return {
      width: data.width,
      height: data.height,
    };
  }
  throw new Error(
    `no successful sootie_screenshot PNG payload found in ${rawLogFile}`,
  );
}

function toolCount(server, logFile) {
  const result = run(server, ["--log-file", logFile, "tools", "--raw"]);
  const tools = JSON.parse(result.stdout);
  if (!Array.isArray(tools)) {
    throw new Error("tools output must be a JSON array");
  }
  return tools.length;
}

function runBuildGate(outputDir, gate, command, commandArgs, env = {}) {
  const result = run(command, commandArgs, { allowFailure: true, env });
  const logFile = path.join(outputDir, `${gate}.log`);
  writeCommandLog(logFile, command, commandArgs, result, env);
  return {
    status: result.status === 0 ? "pass" : "fail",
    artifact: path.basename(logFile),
  };
}

function runBuildGates(args, outputDir) {
  const targetTriple = argValue(args, "--target-triple");
  const targetArgs = targetTriple ? ["--target", targetTriple] : [];
  const targetEnv = parseEnv(repeatedArgValues(args, "--target-env"));
  const commands = {
    cargo_build_workspace: ["cargo", ["build", "--workspace"]],
    cargo_test_workspace: ["cargo", ["test", "--workspace"]],
    cargo_clippy_workspace: [
      "cargo",
      ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
    ],
    target_build: ["cargo", ["build", "--workspace", ...targetArgs], targetEnv],
    cargo_test_no_run: ["cargo", ["test", "--workspace", "--all-targets", "--no-run"]],
    target_clippy: [
      "cargo",
      [
        "clippy",
        "--workspace",
        "--all-targets",
        ...targetArgs,
        "--",
        "-D",
        "warnings",
      ],
      targetEnv,
    ],
    target_test_no_run: [
      "cargo",
      ["test", "--workspace", "--all-targets", ...targetArgs, "--no-run"],
      targetEnv,
    ],
  };
  const status = {};
  const artifacts = {};
  for (const gate of requiredBuildGates) {
    const [command, commandArgs] = commands[gate];
    const result = runBuildGate(outputDir, gate, command, commandArgs, commands[gate][2] || {});
    status[gate] = result.status;
    artifacts[gate] = result.artifact;
  }
  return { status, artifacts };
}

function manualBuildArtifacts(args) {
  const artifacts = {};
  for (const value of repeatedArgValues(args, "--build-artifact")) {
    const [name, artifactPath] = parseKeyValue(value, "--build-artifact");
    if (!requiredBuildGates.includes(name)) {
      throw new Error(`unknown build artifact gate: ${name}`);
    }
    artifacts[name] = artifactPath;
  }
  return artifacts;
}

function buildGateStatus(args, outputDir) {
  if (args.includes("--run-build-gates")) {
    return runBuildGates(args, outputDir);
  }
  const allPassed = args.includes("--build-gates-passed");
  const status = Object.fromEntries(
    requiredBuildGates.map((gate) => [
      gate,
      allPassed ? "pass" : "not_collected",
    ]),
  );
  for (const value of repeatedArgValues(args, "--build-gate")) {
    const [name, gateStatus] = parseKeyValue(value, "--build-gate");
    if (!requiredBuildGates.includes(name)) {
      throw new Error(`unknown build gate: ${name}`);
    }
    status[name] = gateStatus;
  }
  return { status, artifacts: manualBuildArtifacts(args) };
}

function relativeToOutput(outputDir, file) {
  return path.relative(outputDir, file);
}

function smokeArgs({
  output,
  template,
  server,
  runtimeLog,
  timeoutMs,
  envValues,
  framed,
}) {
  const args = [
    "docs/development/run-jsonl-mcp-smoke.mjs",
    "--template",
    template,
    "--output",
    output,
    "--server",
    server,
    "--timeout-ms",
    String(timeoutMs),
    "--arg",
    "--log-file",
    "--arg",
    runtimeLog,
    "--arg",
    "serve",
  ];
  for (const value of envValues) {
    args.push("--env", value);
  }
  if (framed) {
    args.push("--framed");
  }
  return args;
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    return;
  }

  const platform = argValue(args, "--platform", currentPlatform());
  const server = argValue(args, "--server", "target/release/sootie");
  const timeoutMs = Number(argValue(args, "--timeout-ms", "120000"));
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive number");
  }

  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const outputDir = resolveRepoPath(
    argValue(args, "--output-dir", path.join(os.tmpdir(), `sootie-runtime-${platform}-${stamp}`)),
  );
  fs.mkdirSync(outputDir, { recursive: true });
  const buildGates = buildGateStatus(args, outputDir);
  const failedBuildGates = Object.entries(buildGates.status)
    .filter(([, status]) => status === "fail")
    .map(([gate]) => gate);
  if (failedBuildGates.length > 0) {
    throw new Error(
      `build gates failed: ${failedBuildGates.join(", ")}. Logs are in ${outputDir}`,
    );
  }

  if (args.includes("--build-only")) {
    const evidence = {
      platform,
      collected_at: new Date().toISOString(),
      verification_mode: "build-only",
      build: buildGates.status,
      build_artifacts: buildGates.artifacts,
      notes:
        "Build-only evidence. Desktop runtime smokes are intentionally omitted for this platform acceptance scope.",
    };
    const evidenceFile = path.join(outputDir, `${platform}-evidence.json`);
    fs.writeFileSync(evidenceFile, `${JSON.stringify(evidence, null, 2)}\n`);
    console.log(
      JSON.stringify(
        {
          evidence: evidenceFile,
          output_dir: outputDir,
          verifier: `node docs/development/verify-runtime-evidence.mjs --platform ${platform} --build-only ${platform} --evidence ${evidenceFile}`,
        },
        null,
        2,
      ),
    );
    return;
  }

  const envValues = repeatedArgValues(args, "--env");
  const extraEnv = parseEnv(envValues);
  const templateFile = resolveRepoPath(argValue(args, "--template", defaultTemplate));
  const resolvedTemplate = path.join(outputDir, "full-tool-smoke-requests.jsonl");
  writeResolvedTemplate(templateFile, resolvedTemplate, replacementMap(args));

  const doctorJson = path.join(outputDir, "doctor.json");
  const doctorLog = path.join(outputDir, "doctor.log");
  const doctorResult = run(
    server,
    ["--log-file", doctorLog, "doctor", "--check", "--raw"],
    { allowFailure: true, env: extraEnv },
  );
  fs.writeFileSync(doctorJson, doctorResult.stdout);
  const doctor = readJsonFromStdout(doctorResult, "doctor");
  if (doctorResult.status !== 0 && !args.includes("--continue-on-doctor-failure")) {
    throw new Error(
      `doctor --check --raw failed; wrote ${doctorJson}. Re-run with --continue-on-doctor-failure to collect blocked evidence.`,
    );
  }

  const rawJsonRpcLog = path.join(outputDir, "raw-responses.jsonl");
  const framedJsonRpcLog = path.join(outputDir, "framed-responses.jsonl");
  const runtimeLog = path.join(outputDir, "sootie-runtime.log");
  const screenshot = path.join(outputDir, "screenshot.png");
  const framedTemplate = path.join(outputDir, "framed-smoke-requests.jsonl");
  fs.writeFileSync(
    framedTemplate,
    [
      {
        jsonrpc: "2.0",
        id: "framed-tools-list",
        method: "tools/list",
        params: {},
      },
      {
        jsonrpc: "2.0",
        id: "framed-learn-status",
        method: "tools/call",
        params: { name: "sootie_learn_status", arguments: {} },
      },
    ]
      .map((request) => JSON.stringify(request))
      .join("\n")
      .concat("\n"),
  );

  run("node", smokeArgs({
    output: rawJsonRpcLog,
    template: resolvedTemplate,
    server,
    runtimeLog,
    timeoutMs,
    envValues,
    framed: false,
  }));
  run("node", smokeArgs({
    output: framedJsonRpcLog,
    template: framedTemplate,
    server,
    runtimeLog,
    timeoutMs,
    envValues,
    framed: true,
  }));
  run("node", [
    "docs/development/verify-full-tool-smoke.mjs",
    "--responses",
    rawJsonRpcLog,
  ]);

  const screenshotSize = extractScreenshot(rawJsonRpcLog, screenshot);
  const toolsLog = path.join(outputDir, "tools.log");
  const evidence = {
    platform,
    collected_at: new Date().toISOString(),
    build: buildGates.status,
    build_artifacts: buildGates.artifacts,
    runtime: {
      doctor_ready: doctor.runtime_ready === true,
      runtime_blockers: doctor.runtime_blockers || [],
      context_app: doctor.context_app || null,
      context_window: doctor.context_window || null,
      screenshot_size: doctor.screenshot_size || screenshotSize,
    },
    mcp_stdio: {
      line_json: "pass",
      content_length: "pass",
      tool_count: toolCount(server, toolsLog),
    },
    smokes: {
      perception: "pass",
      screenshot: "pass",
      action: "pass",
      recipe: "pass",
      learning: "pass",
      cdp: "pass",
    },
    client_attachment: {
      client: "manual",
      configured: args.includes("--client-configured"),
      fresh_session_tool_call: args.includes("--client-fresh-session-tool-call")
        ? "pass"
        : "not_collected",
    },
    artifacts: {
      doctor_json: relativeToOutput(outputDir, doctorJson),
      raw_json_rpc_log: relativeToOutput(outputDir, rawJsonRpcLog),
      framed_json_rpc_log: relativeToOutput(outputDir, framedJsonRpcLog),
      sootie_runtime_log: relativeToOutput(outputDir, runtimeLog),
      screenshot: relativeToOutput(outputDir, screenshot),
    },
    notes:
      "Generated by collect-runtime-evidence.mjs. Build and client gates must reflect separately captured evidence.",
  };

  const evidenceFile = path.join(outputDir, `${platform}-evidence.json`);
  fs.writeFileSync(evidenceFile, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(
    JSON.stringify(
      {
        evidence: evidenceFile,
        output_dir: outputDir,
        verifier: `node docs/development/verify-runtime-evidence.mjs --platform ${platform} --evidence ${evidenceFile}`,
      },
      null,
      2,
    ),
  );
}

main();
