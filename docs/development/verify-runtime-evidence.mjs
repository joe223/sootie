#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const defaultTemplate = "docs/development/full-tool-smoke-requests.jsonl";
const requiredPlatforms = ["macos", "linux", "windows"];
const requiredBuildGates = [
  "cargo_build_workspace",
  "cargo_test_workspace",
  "cargo_clippy_workspace",
  "target_build",
  "cargo_test_no_run",
  "target_clippy",
  "target_test_no_run",
];
const requiredSmokes = [
  "perception",
  "screenshot",
  "action",
  "recipe",
  "learning",
  "cdp",
];

function usage() {
  console.error(
    [
      "Usage:",
      "  node docs/development/verify-runtime-evidence.mjs --evidence macos.json --evidence linux.json --evidence windows.json",
      "",
      "Options:",
      "  --evidence <path>       Runtime evidence JSON file. Repeatable.",
      "  --platform <name>       Required platform. Repeatable. Defaults to macos, linux, windows.",
      "  --build-only <name>     For this required platform, validate build evidence only.",
    ].join("\n"),
  );
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

function resolveRepoPath(input) {
  return path.isAbsolute(input) ? input : path.join(repoRoot, input);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`${file}: ${error.message}`);
  }
}

function readToolNames() {
  const toolsPath = path.join(repoRoot, "crates/sootie-core/src/tools.rs");
  const source = fs.readFileSync(toolsPath, "utf8");
  const start = source.indexOf("pub const TOOL_NAMES");
  if (start === -1) {
    throw new Error("TOOL_NAMES not found");
  }
  const end = source.indexOf("];", start);
  if (end === -1) {
    throw new Error("TOOL_NAMES block terminator not found");
  }
  return [...source.slice(start, end).matchAll(/"(sootie_[a-z_]+)"/g)].map(
    (match) => match[1],
  );
}

function isPass(value) {
  return value === "pass" || value === true;
}

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function resolveEvidencePath(evidenceFile, artifactPath) {
  return path.isAbsolute(artifactPath)
    ? artifactPath
    : path.join(path.dirname(evidenceFile), artifactPath);
}

function parseJsonLines(file) {
  return fs
    .readFileSync(file, "utf8")
    .split(/\r?\n/)
    .map((line, index) => ({ line: line.trim(), lineNumber: index + 1 }))
    .filter((entry) => entry.line.length > 0)
    .map((entry) => {
      try {
        return { ...entry, value: JSON.parse(entry.line) };
      } catch (error) {
        throw new Error(`${file}:${entry.lineNumber}: ${error.message}`);
      }
    });
}

function duplicateValues(values) {
  const seen = new Set();
  const duplicates = new Set();
  for (const value of values) {
    if (seen.has(value)) {
      duplicates.add(value);
    }
    seen.add(value);
  }
  return [...duplicates].sort();
}

function responseReport(entry) {
  const response = entry.value;
  if (!response.result) {
    return null;
  }
  const structured = response.result.structuredContent || response.result;
  const report = structured && structured.report;
  if (!report || typeof report.tool !== "string") {
    return null;
  }
  const hasValidReport =
    report.success === true && Number.isFinite(report.duration_ms);
  return {
    id: response.id,
    tool: report.tool,
    success:
      structured.success === true &&
      response.result.isError !== true &&
      hasValidReport,
    error:
      structured.error ||
      report.error ||
      (report.success !== true ? "report.success is not true" : null) ||
      (!Number.isFinite(report.duration_ms)
        ? "report.duration_ms is not a finite number"
        : null),
    lineNumber: entry.lineNumber,
  };
}

function readTemplateToolById() {
  const templateFile = path.join(repoRoot, defaultTemplate);
  const byId = new Map();
  for (const entry of parseJsonLines(templateFile)) {
    const request = entry.value;
    const id = request.id;
    const name = request.params && request.params.name;
    if (request.method !== "tools/call" || typeof name !== "string") {
      throw new Error(
        `${templateFile}:${entry.lineNumber}: expected tools/call with params.name`,
      );
    }
    if (id === undefined || id === null) {
      throw new Error(`${templateFile}:${entry.lineNumber}: request id is required`);
    }
    const key = String(id);
    if (byId.has(key)) {
      throw new Error(`${templateFile}:${entry.lineNumber}: duplicate request id ${key}`);
    }
    byId.set(key, name);
  }
  return byId;
}

function compareTemplateResponses(templateById, reports) {
  const byId = new Map();
  for (const report of reports) {
    if (report.id === undefined || report.id === null) {
      continue;
    }
    byId.set(String(report.id), report);
  }
  const missing_template_responses = [];
  const mismatched_pairs = [];
  for (const [id, expectedTool] of templateById) {
    const report = byId.get(id);
    if (!report) {
      missing_template_responses.push({ id, expected_tool: expectedTool });
      continue;
    }
    if (report.tool !== expectedTool) {
      mismatched_pairs.push({
        id: report.id,
        expected_tool: expectedTool,
        report_tool: report.tool,
        line: report.lineNumber,
      });
    }
  }
  return {
    missing_template_responses,
    mismatched_pairs,
  };
}

function verifyRawJsonRpcLog(file, expectedTools, templateById) {
  const reports = parseJsonLines(file).map(responseReport).filter(Boolean);
  const tools = reports.map((report) => report.tool);
  const expectedSet = new Set(expectedTools);
  const seenSet = new Set(tools);
  const missing = expectedTools.filter((tool) => !seenSet.has(tool));
  const extra = tools.filter((tool) => !expectedSet.has(tool));
  const failed = reports
    .filter((report) => !report.success)
    .map((report) => ({
      tool: report.tool,
      line: report.lineNumber,
      error: report.error,
    }));
  const duplicate_response_ids = duplicateValues(
    reports
      .filter((report) => report.id !== undefined && report.id !== null)
      .map((report) => String(report.id)),
  );
  const responsePairing = compareTemplateResponses(templateById, reports);
  return {
    file: path.relative(repoRoot, file),
    response_count: reports.length,
    matched_tool_count: expectedTools.filter((tool) => seenSet.has(tool)).length,
    missing,
    extra: [...new Set(extra)].sort(),
    duplicate_response_ids,
    missing_template_responses: responsePairing.missing_template_responses,
    mismatched_pairs: responsePairing.mismatched_pairs,
    failed,
    success:
      missing.length === 0 &&
      extra.length === 0 &&
      duplicate_response_ids.length === 0 &&
      responsePairing.missing_template_responses.length === 0 &&
      responsePairing.mismatched_pairs.length === 0 &&
      failed.length === 0,
  };
}

function verifyFramedJsonRpcLog(file, expectedToolCount) {
  const entries = parseJsonLines(file);
  const reports = entries.map(responseReport).filter(Boolean);
  const learnStatus = reports.find(
    (report) => report.tool === "learn_status",
  );
  const toolsListResponse = entries.find(
    (entry) => entry.value.id === "framed-tools-list",
  )?.value;
  const tools = toolsListResponse?.result?.tools;
  const learnStatusTool = Array.isArray(tools)
    ? tools.find((tool) => tool?.name === "learn_status")
    : null;
  const ids = entries
    .map((entry) => entry.value.id)
    .filter((id) => id !== undefined && id !== null)
    .map((id) => String(id));
  const duplicate_response_ids = duplicateValues(ids);
  const errors = [];
  if (entries.length < 3) {
    errors.push("framed log must include initialize, tools/list, and tool-call responses");
  }
  if (!entries.some((entry) => entry.value.id === "smoke-init")) {
    errors.push("framed log must include smoke-init initialize response");
  }
  if (!toolsListResponse) {
    errors.push("framed log must include framed-tools-list response");
  } else if (!Array.isArray(tools)) {
    errors.push("framed tools/list response must include tools array");
  } else if (tools.length !== expectedToolCount) {
    errors.push(
      `framed tools/list must return ${expectedToolCount} tools, got ${tools.length}`,
    );
  }
  if (!learnStatusTool) {
    errors.push("framed tools/list must include learn_status");
  } else if (learnStatusTool.annotations?.readOnlyHint !== true) {
    errors.push(
      "framed tools/list must mark learn_status readOnlyHint true",
    );
  }
  if (!learnStatus) {
    errors.push("framed log must include learn_status report");
  } else if (!learnStatus.success) {
    errors.push("framed learn_status report must be successful");
  }
  if (duplicate_response_ids.length > 0) {
    errors.push(
      `framed log has duplicate response ids: ${duplicate_response_ids.join(", ")}`,
    );
  }
  return {
    file: path.relative(repoRoot, file),
    success: errors.length === 0,
    errors,
    response_count: entries.length,
    duplicate_response_ids,
    tool_count: Array.isArray(tools) ? tools.length : null,
    learn_status_read_only:
      learnStatusTool?.annotations?.readOnlyHint === true,
    learn_status_success: learnStatus?.success === true,
  };
}

function verifyDoctorJson(file, evidence) {
  const doctor = readJson(file);
  const errors = [];
  if (doctor.platform !== evidence.platform) {
    errors.push(`platform mismatch: expected ${evidence.platform}, got ${doctor.platform}`);
  }
  if (doctor.runtime_ready !== true) {
    errors.push("runtime_ready must be true");
  }
  if (!Array.isArray(doctor.runtime_blockers)) {
    errors.push("runtime_blockers must be an array");
  } else if (doctor.runtime_blockers.length > 0) {
    errors.push("runtime_blockers must be empty");
  }
  if (doctor.context_app !== evidence.runtime?.context_app) {
    errors.push(
      `context_app mismatch: expected ${evidence.runtime?.context_app}, got ${doctor.context_app}`,
    );
  }
  if (doctor.context_window !== evidence.runtime?.context_window) {
    errors.push(
      `context_window mismatch: expected ${evidence.runtime?.context_window}, got ${doctor.context_window}`,
    );
  }
  if (doctor.screenshot_available !== true) {
    errors.push("screenshot_available must be true");
  }
  const doctorWidth = doctor.screenshot_size?.width;
  const doctorHeight = doctor.screenshot_size?.height;
  if (
    !Number.isFinite(doctorWidth) ||
    doctorWidth <= 0 ||
    !Number.isFinite(doctorHeight) ||
    doctorHeight <= 0
  ) {
    errors.push("screenshot_size must have positive width and height");
  }
  if (
    doctorWidth !== evidence.runtime?.screenshot_size?.width ||
    doctorHeight !== evidence.runtime?.screenshot_size?.height
  ) {
    errors.push("screenshot_size must match runtime summary");
  }
  if (
    Array.isArray(doctor.runtime_diagnostics) &&
    doctor.runtime_diagnostics.some((diagnostic) => diagnostic?.success === false)
  ) {
    errors.push("runtime_diagnostics must not include failed diagnostics");
  }
  return {
    file: path.relative(repoRoot, file),
    success: errors.length === 0,
    errors,
    platform: doctor.platform,
    runtime_ready: doctor.runtime_ready,
    context_app: doctor.context_app,
    context_window: doctor.context_window,
    screenshot_size: doctor.screenshot_size || null,
    runtime_blockers: doctor.runtime_blockers || null,
  };
}

function pngDimensions(file) {
  const bytes = fs.readFileSync(file);
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  if (bytes.length < 24 || !bytes.subarray(0, 8).equals(signature)) {
    throw new Error("not a PNG file");
  }
  if (bytes.subarray(12, 16).toString("ascii") !== "IHDR") {
    throw new Error("missing PNG IHDR chunk");
  }
  const width = bytes.readUInt32BE(16);
  const height = bytes.readUInt32BE(20);
  if (width <= 0 || height <= 0) {
    throw new Error("PNG dimensions must be positive");
  }
  return { width, height };
}

function verifyScreenshot(file, evidence) {
  const errors = [];
  let size = null;
  try {
    size = pngDimensions(file);
  } catch (error) {
    errors.push(error.message);
  }
  if (
    size &&
    (size.width !== evidence.runtime?.screenshot_size?.width ||
      size.height !== evidence.runtime?.screenshot_size?.height)
  ) {
    errors.push(
      `PNG dimensions ${size.width}x${size.height} do not match runtime summary`,
    );
  }
  return {
    file: path.relative(repoRoot, file),
    success: errors.length === 0,
    errors,
    size,
  };
}

function verifyRuntimeLog(file) {
  const size = fs.statSync(file).size;
  return {
    file: path.relative(repoRoot, file),
    success: size > 0,
    errors: size > 0 ? [] : ["runtime log must be non-empty"],
    size_bytes: size,
  };
}

function verifyBuildArtifact(file) {
  const content = fs.readFileSync(file, "utf8");
  const errors = [];
  if (content.trim().length === 0) {
    errors.push("build artifact log must be non-empty");
  }
  if (!content.includes("exit_status=0")) {
    errors.push("build artifact log must include exit_status=0");
  }
  return {
    file: path.relative(repoRoot, file),
    success: errors.length === 0,
    errors,
    size_bytes: Buffer.byteLength(content),
  };
}

function fileExists(file) {
  try {
    return fs.statSync(file).isFile();
  } catch (_) {
    return false;
  }
}

function validateEvidence(file, evidence, expectedTools, templateById, buildOnlyPlatforms) {
  const errors = [];
  const artifact_checks = {};
  const platform = evidence.platform;
  const mode = buildOnlyPlatforms.has(platform) ? "build-only" : "runtime";
  const expectedToolCount = expectedTools.length;
  if (!requiredPlatforms.includes(platform)) {
    errors.push(`platform must be one of ${requiredPlatforms.join(", ")}`);
  }

  const build_artifacts = {};
  const buildArtifacts = evidence.build_artifacts || {};
  for (const gate of requiredBuildGates) {
    if (!isPass(evidence.build?.[gate])) {
      errors.push(`build.${gate} must be pass`);
      continue;
    }
    if (!isNonEmptyString(buildArtifacts[gate])) {
      errors.push(`build_artifacts.${gate} must be present`);
      continue;
    }
    const buildArtifactFile = resolveEvidencePath(file, buildArtifacts[gate]);
    if (!fileExists(buildArtifactFile)) {
      errors.push(`build_artifacts.${gate} file must exist: ${buildArtifacts[gate]}`);
      build_artifacts[gate] = {
        file: path.relative(repoRoot, buildArtifactFile),
        exists: false,
      };
      continue;
    }
    build_artifacts[gate] = verifyBuildArtifact(buildArtifactFile);
    for (const error of build_artifacts[gate].errors) {
      errors.push(`build_artifacts.${gate} ${error}`);
    }
  }

  if (mode === "build-only") {
    return {
      file: path.relative(repoRoot, file),
      platform,
      mode,
      success: errors.length === 0,
      errors,
      build_artifacts,
      artifact_checks,
      doctor_json: null,
      runtime_log: null,
      screenshot: null,
      raw_json_rpc_log: null,
      framed_json_rpc_log: null,
    };
  }

  if (evidence.runtime?.doctor_ready !== true) {
    errors.push("runtime.doctor_ready must be true");
  }
  if (!Array.isArray(evidence.runtime?.runtime_blockers)) {
    errors.push("runtime.runtime_blockers must be an array");
  } else if (evidence.runtime.runtime_blockers.length > 0) {
    errors.push("runtime.runtime_blockers must be empty");
  }
  if (
    !isNonEmptyString(evidence.runtime?.context_app) ||
    evidence.runtime.context_app === "unknown" ||
    evidence.runtime.context_app === "loginwindow"
  ) {
    errors.push("runtime.context_app must be a real app, not unknown/loginwindow");
  }
  if (!isNonEmptyString(evidence.runtime?.context_window)) {
    errors.push("runtime.context_window must be present");
  }
  if (
    !Number.isFinite(evidence.runtime?.screenshot_size?.width) ||
    evidence.runtime.screenshot_size.width <= 0 ||
    !Number.isFinite(evidence.runtime?.screenshot_size?.height) ||
    evidence.runtime.screenshot_size.height <= 0
  ) {
    errors.push("runtime.screenshot_size must have positive width and height");
  }

  if (!isPass(evidence.mcp_stdio?.line_json)) {
    errors.push("mcp_stdio.line_json must be pass");
  }
  if (!isPass(evidence.mcp_stdio?.content_length)) {
    errors.push("mcp_stdio.content_length must be pass");
  }
  if (evidence.mcp_stdio?.tool_count !== expectedToolCount) {
    errors.push(`mcp_stdio.tool_count must be ${expectedToolCount}`);
  }

  for (const smoke of requiredSmokes) {
    if (!isPass(evidence.smokes?.[smoke])) {
      errors.push(`smokes.${smoke} must be pass`);
    }
  }

  if (evidence.client_attachment?.configured !== true) {
    errors.push("client_attachment.configured must be true");
  }
  if (!isPass(evidence.client_attachment?.fresh_session_tool_call)) {
    errors.push("client_attachment.fresh_session_tool_call must be pass");
  }

  const artifacts = evidence.artifacts || {};
  for (const name of [
    "doctor_json",
    "raw_json_rpc_log",
    "framed_json_rpc_log",
    "runtime_log",
    "screenshot",
  ]) {
    if (!isNonEmptyString(artifacts[name])) {
      errors.push(`artifacts.${name} must be present`);
      continue;
    }
    const artifactFile = resolveEvidencePath(file, artifacts[name]);
    artifact_checks[name] = {
      file: path.relative(repoRoot, artifactFile),
      exists: fileExists(artifactFile),
    };
    if (!artifact_checks[name].exists) {
      errors.push(`artifacts.${name} file must exist: ${artifacts[name]}`);
    }
  }

  let doctor_json = null;
  const doctorPath = artifacts.doctor_json;
  if (isNonEmptyString(doctorPath)) {
    const doctorFile = resolveEvidencePath(file, doctorPath);
    if (fileExists(doctorFile)) {
      doctor_json = verifyDoctorJson(doctorFile, evidence);
      for (const error of doctor_json.errors) {
        errors.push(`artifacts.doctor_json ${error}`);
      }
    }
  }

  let runtime_log = null;
  const runtimeLogPath = artifacts.runtime_log;
  if (isNonEmptyString(runtimeLogPath)) {
    const runtimeLogFile = resolveEvidencePath(file, runtimeLogPath);
    if (fileExists(runtimeLogFile)) {
      runtime_log = verifyRuntimeLog(runtimeLogFile);
      for (const error of runtime_log.errors) {
        errors.push(`artifacts.runtime_log ${error}`);
      }
    }
  }

  let screenshot = null;
  const screenshotPath = artifacts.screenshot;
  if (isNonEmptyString(screenshotPath)) {
    const screenshotFile = resolveEvidencePath(file, screenshotPath);
    if (fileExists(screenshotFile)) {
      screenshot = verifyScreenshot(screenshotFile, evidence);
      for (const error of screenshot.errors) {
        errors.push(`artifacts.screenshot ${error}`);
      }
    }
  }

  let raw_json_rpc_log = null;
  const rawLogPath = artifacts.raw_json_rpc_log;
  if (isNonEmptyString(rawLogPath)) {
    const rawLogFile = resolveEvidencePath(file, rawLogPath);
    if (fileExists(rawLogFile)) {
      raw_json_rpc_log = verifyRawJsonRpcLog(
        rawLogFile,
        expectedTools,
        templateById,
      );
      if (!raw_json_rpc_log.success) {
        if (raw_json_rpc_log.missing.length > 0) {
          errors.push(
            `artifacts.raw_json_rpc_log missing tools: ${raw_json_rpc_log.missing.join(", ")}`,
          );
        }
        if (raw_json_rpc_log.extra.length > 0) {
          errors.push(
            `artifacts.raw_json_rpc_log has unknown tools: ${raw_json_rpc_log.extra.join(", ")}`,
          );
        }
        if (raw_json_rpc_log.duplicate_response_ids.length > 0) {
          errors.push(
            `artifacts.raw_json_rpc_log has duplicate response ids: ${raw_json_rpc_log.duplicate_response_ids.join(", ")}`,
          );
        }
        if (raw_json_rpc_log.missing_template_responses.length > 0) {
          errors.push(
            `artifacts.raw_json_rpc_log missing template response ids: ${raw_json_rpc_log.missing_template_responses
              .map((missing) => missing.id)
              .join(", ")}`,
          );
        }
        if (raw_json_rpc_log.mismatched_pairs.length > 0) {
          errors.push(
            `artifacts.raw_json_rpc_log has mismatched template pairs: ${raw_json_rpc_log.mismatched_pairs
              .map(
                (pair) =>
                  `${pair.id}:${pair.expected_tool}->${pair.report_tool}`,
              )
              .join(", ")}`,
          );
        }
        if (raw_json_rpc_log.failed.length > 0) {
          errors.push(
            `artifacts.raw_json_rpc_log has failed tool reports: ${raw_json_rpc_log.failed
              .map((failure) => failure.tool)
              .join(", ")}`,
          );
        }
      }
    }
  }

  let framed_json_rpc_log = null;
  const framedLogPath = artifacts.framed_json_rpc_log;
  if (isNonEmptyString(framedLogPath)) {
    const framedLogFile = resolveEvidencePath(file, framedLogPath);
    if (fileExists(framedLogFile)) {
      framed_json_rpc_log = verifyFramedJsonRpcLog(
        framedLogFile,
        expectedToolCount,
      );
      for (const error of framed_json_rpc_log.errors) {
        errors.push(`artifacts.framed_json_rpc_log ${error}`);
      }
    }
  }

  return {
    file: path.relative(repoRoot, file),
    platform,
    mode,
    success: errors.length === 0,
    errors,
    build_artifacts,
    artifact_checks,
    doctor_json,
    runtime_log,
    screenshot,
    raw_json_rpc_log,
    framed_json_rpc_log,
  };
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    return;
  }

  const evidenceFiles = repeatedArgValues(args, "--evidence").map(resolveRepoPath);
  const platforms = repeatedArgValues(args, "--platform");
  const expectedPlatforms = platforms.length > 0 ? platforms : requiredPlatforms;
  const buildOnlyPlatforms = new Set(repeatedArgValues(args, "--build-only"));
  if (evidenceFiles.length === 0) {
    usage();
    throw new Error("--evidence is required");
  }
  for (const platform of expectedPlatforms) {
    if (!requiredPlatforms.includes(platform)) {
      throw new Error(`unsupported --platform value: ${platform}`);
    }
  }
  for (const platform of buildOnlyPlatforms) {
    if (!requiredPlatforms.includes(platform)) {
      throw new Error(`unsupported --build-only value: ${platform}`);
    }
    if (!expectedPlatforms.includes(platform)) {
      throw new Error(`--build-only platform must also be required by --platform: ${platform}`);
    }
  }

  const expectedTools = readToolNames();
  const templateById = readTemplateToolById();
  const entries = evidenceFiles.map((file) =>
    validateEvidence(
      file,
      readJson(file),
      expectedTools,
      templateById,
      buildOnlyPlatforms,
    ),
  );
  const platformsSeen = new Map();
  const duplicatePlatforms = [];
  for (const entry of entries) {
    if (platformsSeen.has(entry.platform)) {
      duplicatePlatforms.push(entry.platform);
    }
    platformsSeen.set(entry.platform, entry.file);
  }
  const missingPlatforms = expectedPlatforms.filter(
    (platform) => !platformsSeen.has(platform),
  );

  const output = {
    expected_tool_count: expectedTools.length,
    required_platforms: expectedPlatforms,
    build_only_platforms: [...buildOnlyPlatforms].sort(),
    missing_platforms: missingPlatforms,
    duplicate_platforms: [...new Set(duplicatePlatforms)].sort(),
    evidence: entries,
    success:
      missingPlatforms.length === 0 &&
      duplicatePlatforms.length === 0 &&
      entries.every((entry) => entry.success),
  };

  console.log(JSON.stringify(output, null, 2));
  process.exitCode = output.success ? 0 : 1;
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
