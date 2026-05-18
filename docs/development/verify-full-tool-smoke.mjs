#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const defaultTemplate = "docs/development/full-tool-smoke-requests.jsonl";

function usage() {
  console.error(
    [
      "Usage:",
      "  node docs/development/verify-full-tool-smoke.mjs",
      "  node docs/development/verify-full-tool-smoke.mjs --responses path/to/responses.jsonl",
      "",
      "Options:",
      "  --template <path>       JSONL request template to check.",
      "  --responses <path>      JSONL MCP responses captured from a smoke run.",
      "  --allow-failures        Summarize responses even if success is not true.",
    ].join("\n"),
  );
}

function readArgValue(args, name) {
  const index = args.indexOf(name);
  if (index === -1) {
    return null;
  }
  if (index + 1 >= args.length) {
    throw new Error(`${name} requires a path`);
  }
  return args[index + 1];
}

function resolveRepoPath(input) {
  return path.isAbsolute(input) ? input : path.join(repoRoot, input);
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

function compareSets(expected, actual) {
  const actualSet = new Set(actual);
  const expectedSet = new Set(expected);
  return {
    missing: expected.filter((name) => !actualSet.has(name)),
    extra: actual.filter((name) => !expectedSet.has(name)),
    duplicates: duplicateValues(actual),
  };
}

function compareTemplateResponses(templateById, reports) {
  const byId = new Map();
  const duplicate_response_ids = [];
  for (const report of reports) {
    if (report.id === undefined || report.id === null) {
      continue;
    }
    const key = String(report.id);
    if (byId.has(key)) {
      duplicate_response_ids.push(report.id);
    }
    byId.set(key, report);
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
    duplicate_response_ids,
    missing_template_responses,
    mismatched_pairs,
  };
}

function learningOrderErrors(templateTools) {
  const start = templateTools.indexOf("sootie_learn_start");
  const status = templateTools.indexOf("sootie_learn_status");
  const stop = templateTools.indexOf("sootie_learn_stop");
  const recordableTools = new Set([
    "sootie_click",
    "sootie_type",
    "sootie_press",
    "sootie_hotkey",
    "sootie_scroll",
    "sootie_hover",
    "sootie_long_press",
    "sootie_drag",
    "sootie_focus",
    "sootie_window",
    "sootie_wait",
  ]);
  const errors = [];
  if (start === -1 || status === -1 || stop === -1) {
    errors.push("learning start/status/stop tools must all be present");
    return errors;
  }
  if (!(start < status && status < stop)) {
    errors.push("learning status and stop must run after learning start");
  }
  const recordableBetweenStartAndStatus = templateTools
    .slice(start + 1, status)
    .some((tool) => recordableTools.has(tool));
  if (!recordableBetweenStartAndStatus) {
    errors.push("at least one recordable action must run before learning status");
  }
  return errors;
}

function templateToolName(entry, templateFile) {
  const request = entry.value;
  const name = request.params && request.params.name;
  if (request.method !== "tools/call" || typeof name !== "string") {
    throw new Error(
      `${templateFile}:${entry.lineNumber}: expected tools/call with params.name`,
    );
  }
  return name;
}

function templateToolById(entries, templateFile) {
  const byId = new Map();
  for (const entry of entries) {
    templateToolName(entry, templateFile);
    const id = entry.value.id;
    if (id === undefined || id === null) {
      throw new Error(`${templateFile}:${entry.lineNumber}: expected request id`);
    }
    const key = String(id);
    if (byId.has(key)) {
      throw new Error(`${templateFile}:${entry.lineNumber}: duplicate request id ${key}`);
    }
    byId.set(key, entry.value.params.name);
  }
  return byId;
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

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    return;
  }

  const templateFile = resolveRepoPath(
    readArgValue(args, "--template") || defaultTemplate,
  );
  const responseFileArg = readArgValue(args, "--responses");
  const allowFailures = args.includes("--allow-failures");

  const expectedTools = readToolNames();
  const templateEntries = parseJsonLines(templateFile);
  const templateTools = templateEntries.map((entry) =>
    templateToolName(entry, templateFile),
  );
  const templateById = templateToolById(templateEntries, templateFile);
  const templateCompare = compareSets(expectedTools, templateTools);
  const learning_errors = learningOrderErrors(templateTools);

  const output = {
    template: {
      file: path.relative(repoRoot, templateFile),
      tool_count: expectedTools.length,
      request_count: templateTools.length,
      ...templateCompare,
      learning_errors,
    },
  };

  let exitCode = 0;
  if (
    templateCompare.missing.length > 0 ||
    templateCompare.extra.length > 0 ||
    templateCompare.duplicates.length > 0 ||
    learning_errors.length > 0
  ) {
    exitCode = 1;
  }

  if (responseFileArg) {
    const responseFile = resolveRepoPath(responseFileArg);
    const reports = parseJsonLines(responseFile)
      .map(responseReport)
      .filter(Boolean);
    const responseTools = reports.map((report) => report.tool);
    const responseCompare = compareSets(expectedTools, responseTools);
    const matchedTools = expectedTools.filter((tool) =>
      responseTools.includes(tool),
    );
    const failed = reports
      .filter((report) => !report.success)
      .map((report) => ({
        tool: report.tool,
        line: report.lineNumber,
        error: report.error,
      }));
    const responsePairing = compareTemplateResponses(templateById, reports);

    output.responses = {
      file: path.relative(repoRoot, responseFile),
      response_count: reports.length,
      matched_tool_count: matchedTools.length,
      missing: responseCompare.missing,
      extra: responseCompare.extra,
      repeated_tools: responseCompare.duplicates,
      duplicate_response_ids: responsePairing.duplicate_response_ids,
      missing_template_responses: responsePairing.missing_template_responses,
      mismatched_pairs: responsePairing.mismatched_pairs,
      failed,
    };

    if (
      responseCompare.missing.length > 0 ||
      responseCompare.extra.length > 0 ||
      responsePairing.duplicate_response_ids.length > 0 ||
      responsePairing.missing_template_responses.length > 0 ||
      responsePairing.mismatched_pairs.length > 0 ||
      (!allowFailures && failed.length > 0)
    ) {
      exitCode = 1;
    }
  }

  console.log(JSON.stringify(output, null, 2));
  process.exitCode = exitCode;
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
