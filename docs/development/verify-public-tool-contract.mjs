#!/usr/bin/env node

import { execFileSync } from "node:child_process";

const expectedTools = [
  ["sootie_context", { app: "string" }, []],
  ["sootie_state", { app: "string" }, []],
  [
    "sootie_find",
    {
      app: "string",
      depth: "integer",
      dom_class: "string",
      dom_id: "string",
      identifier: "string",
      query: "string",
      role: "string",
    },
    [],
  ],
  ["sootie_read", { app: "string", depth: "integer", query: "string" }, []],
  [
    "sootie_inspect",
    { app: "string", dom_id: "string", query: "string", role: "string" },
    ["query"],
  ],
  ["sootie_element_at", { x: "number", y: "number" }, ["x", "y"]],
  [
    "sootie_screenshot",
    { app: "string", full_resolution: "boolean" },
    [],
  ],
  [
    "sootie_click",
    {
      app: "string",
      button: "string",
      count: "integer",
      dom_id: "string",
      query: "string",
      role: "string",
      x: "number",
      y: "number",
    },
    [],
  ],
  [
    "sootie_type",
    {
      app: "string",
      clear: "boolean",
      dom_id: "string",
      into: "string",
      text: "string",
    },
    ["text"],
  ],
  [
    "sootie_press",
    { app: "string", key: "string", modifiers: "array:string" },
    ["key"],
  ],
  ["sootie_hotkey", { app: "string", keys: "array:string" }, ["keys"]],
  [
    "sootie_scroll",
    {
      amount: "integer",
      app: "string",
      direction: "string",
      x: "number",
      y: "number",
    },
    ["direction"],
  ],
  [
    "sootie_hover",
    {
      app: "string",
      dom_id: "string",
      query: "string",
      role: "string",
      x: "number",
      y: "number",
    },
    [],
  ],
  [
    "sootie_long_press",
    {
      app: "string",
      button: "string",
      dom_id: "string",
      duration: "number",
      query: "string",
      role: "string",
      x: "number",
      y: "number",
    },
    [],
  ],
  [
    "sootie_drag",
    {
      app: "string",
      dom_id: "string",
      duration: "number",
      from_x: "number",
      from_y: "number",
      hold_duration: "number",
      query: "string",
      role: "string",
      to_x: "number",
      to_y: "number",
    },
    ["to_x", "to_y"],
  ],
  ["sootie_focus", { app: "string", window: "string" }, ["app"]],
  [
    "sootie_window",
    {
      action: "string",
      app: "string",
      height: "number",
      width: "number",
      window: "string",
      x: "number",
      y: "number",
    },
    ["action", "app"],
  ],
  [
    "sootie_wait",
    {
      app: "string",
      condition: "string",
      interval: "number",
      timeout: "number",
      value: "string",
    },
    ["condition"],
  ],
  ["sootie_recipes", {}, []],
  ["sootie_run", { params: "object", recipe: "string" }, ["recipe"]],
  ["sootie_recipe_show", { name: "string" }, ["name"]],
  ["sootie_recipe_save", { recipe_json: "string" }, ["recipe_json"]],
  ["sootie_recipe_delete", { name: "string" }, ["name"]],
  [
    "sootie_parse_screen",
    { app: "string", full_resolution: "boolean" },
    [],
  ],
  [
    "sootie_ground",
    { app: "string", crop_box: "array:number", description: "string" },
    ["description"],
  ],
  [
    "sootie_annotate",
    { app: "string", max_labels: "integer", roles: "array:string" },
    [],
  ],
  ["sootie_learn_start", { task_description: "string" }, []],
  ["sootie_learn_stop", {}, []],
  ["sootie_learn_status", {}, []],
];

const forbiddenPublicFields = [
  "target",
  "from_target",
  "to_target",
  "el_description",
  "platform_app_id",
  "to_platform_app_id",
  "bundle_id",
  "to_bundle_id",
  "to_app",
  "clear_first",
  "duration_ms",
  "hold_duration_ms",
  "timeout_ms",
  "interval_ms",
  "bounds",
  "max_candidates",
];

function argValue(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) {
    return fallback;
  }
  if (index + 1 >= args.length) {
    throw new Error(`${name} requires a value`);
  }
  return args[index + 1];
}

function schemaType(schema) {
  const type = schema && schema.type;
  if (type !== "array") {
    return type;
  }
  return `array:${schema.items && schema.items.type}`;
}

function compareArrays(label, actual, expected, errors) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    errors.push(`${label}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    console.log(
      [
        "Usage:",
        "  node docs/development/verify-public-tool-contract.mjs",
        "",
        "Options:",
        "  --server <path>  Sootie binary. Default: target/release/sootie",
      ].join("\n"),
    );
    return;
  }

  const server = argValue(args, "--server", "target/release/sootie");
  const tools = JSON.parse(execFileSync(server, ["tools", "--raw"], { encoding: "utf8" }));
  const expectedNames = expectedTools.map(([name]) => name);
  const actualNames = tools.map((tool) => tool.name);
  const errors = [];

  compareArrays("tool names", actualNames, expectedNames, errors);

  for (const [name, expectedProperties, expectedRequired] of expectedTools) {
    const tool = tools.find((item) => item.name === name);
    if (!tool) {
      errors.push(`${name}: missing tool`);
      continue;
    }
    const properties = tool.inputSchema && tool.inputSchema.properties;
    const actualProperties = Object.keys(properties || {}).sort();
    const expectedPropertyNames = Object.keys(expectedProperties).sort();
    compareArrays(`${name} properties`, actualProperties, expectedPropertyNames, errors);

    const actualRequired = tool.inputSchema.required || [];
    compareArrays(`${name} required`, actualRequired, expectedRequired, errors);

    for (const property of expectedPropertyNames) {
      const actualType = schemaType(properties[property]);
      const expectedType = expectedProperties[property];
      if (actualType !== expectedType) {
        errors.push(`${name}.${property}: expected ${expectedType}, got ${actualType}`);
      }
    }

    for (const property of forbiddenPublicFields) {
      if (Object.hasOwn(properties || {}, property)) {
        errors.push(`${name}: unexpectedly advertises ${property}`);
      }
    }
    if (name !== "sootie_ground" && Object.hasOwn(properties || {}, "description")) {
      errors.push(`${name}: unexpectedly advertises description`);
    }
  }

  const result = {
    server,
    tool_count: tools.length,
    expected_tool_count: expectedTools.length,
    errors,
  };
  console.log(JSON.stringify(result, null, 2));
  if (errors.length > 0) {
    process.exitCode = 1;
  }
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
