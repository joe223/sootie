#!/usr/bin/env node

import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const forbiddenTerms = [
  String.fromCharCode(103, 104, 111, 115, 116),
];
const ignoredDirectoryNames = new Set([
  ".git",
  ".omx",
  "target",
  "node_modules",
]);
const ignoredFiles = new Set(["Cargo.lock"]);

function walk(dir, paths = [], files = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") && entry.name !== ".mcp.json") {
      if (ignoredDirectoryNames.has(entry.name)) {
        continue;
      }
    }

    const fullPath = path.join(dir, entry.name);
    const relativePath = path.relative(root, fullPath);
    paths.push(relativePath);

    if (entry.isDirectory()) {
      if (ignoredDirectoryNames.has(entry.name)) {
        continue;
      }
      walk(fullPath, paths, files);
      continue;
    }

    if (!entry.isFile() || ignoredFiles.has(entry.name)) {
      continue;
    }

    files.push(relativePath);
  }
  return { paths, files };
}

function findForbiddenTerm(value) {
  const normalized = value.toLowerCase();
  return forbiddenTerms.find((term) => normalized.includes(term));
}

const matches = [];
const { paths, files } = walk(root);
for (const entryPath of paths) {
  if (findForbiddenTerm(entryPath)) {
    matches.push({ kind: "path", path: entryPath });
  }
}

for (const file of files) {
  const fullPath = path.join(root, file);
  if (statSync(fullPath).size > 8 * 1024 * 1024) {
    continue;
  }

  let content;
  try {
    content = readFileSync(fullPath, "utf8");
  } catch {
    continue;
  }

  for (const term of forbiddenTerms) {
    const index = content.toLowerCase().indexOf(term);
    if (index !== -1) {
      const line = content.slice(0, index).split("\n").length;
      matches.push({ kind: "content", path: file, line });
      break;
    }
  }
}

if (matches.length > 0) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        errors: matches,
      },
      null,
      2,
    ),
  );
  process.exit(1);
}

console.log(
  JSON.stringify(
    {
      ok: true,
      scanned_paths: paths.length,
      scanned_files: files.length,
      errors: [],
    },
    null,
    2,
  ),
);
