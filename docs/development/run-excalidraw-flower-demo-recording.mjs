#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";

const repoRoot = process.cwd();

function usage() {
  console.error(
    [
      "Usage:",
      "  node docs/development/run-excalidraw-flower-demo-recording.mjs",
      "",
      "Options:",
      "  --recipe <name>        Recipe to run. Default: name from --recipe-file",
      "  --recipe-file <path>   Recipe JSON to install before running. Default: docs/development/recipes/safari-excalidraw-human-actions-red-flower.recipe.json",
      "  --server <path>        Sootie binary. Default: $SOOTIE_DEMO_SERVER, target/debug/sootie, then target/release/sootie.",
      "  --output-dir <path>    Demo output directory. Default: ~/Desktop/sootie/demo",
      "  --output <name>        MP4 filename. Default: sootie-excalidraw-human-actions-red-flower-fullscreen-demo.mp4",
      "  --final-image <name>   Final screenshot filename. Default: sootie-excalidraw-human-actions-red-flower-final.png",
      "  --duration <seconds>   Fullscreen recording duration. Default: 45",
      "  --display <id>         screencapture display id. Default: 1",
      "  --check-only           Check lock state and required commands, then exit.",
      "  --dry-run              Print resolved plan without running Sootie or recording.",
      "  --skip-window-prepare  Do not activate and size Safari before recording.",
      "  --postprocess-only     Reuse existing raw recording and responses to write MP4/final image.",
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

function expandHome(input) {
  if (input === "~") {
    return os.homedir();
  }
  if (input.startsWith("~/")) {
    return path.join(os.homedir(), input.slice(2));
  }
  return input;
}

function resolveRepoPath(input) {
  const expanded = expandHome(input);
  return path.isAbsolute(expanded) ? expanded : path.join(repoRoot, expanded);
}

function defaultServer() {
  if (process.env.SOOTIE_DEMO_SERVER) {
    return process.env.SOOTIE_DEMO_SERVER;
  }
  if (fs.existsSync("target/debug/sootie")) {
    return "target/debug/sootie";
  }
  return "target/release/sootie";
}

function commandExists(command) {
  if (!/^[A-Za-z0-9._/-]+$/.test(command)) {
    return false;
  }
  const result = spawnSync("sh", ["-lc", `command -v ${command}`], {
    encoding: "utf8",
  });
  return result.status === 0;
}

function macosScreenLocked() {
  const result = spawnSync("ioreg", ["-n", "Root", "-d1"], {
    encoding: "utf8",
  });
  if (result.status !== 0) {
    return null;
  }
  for (const rawLine of result.stdout.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line.includes("IOConsoleLocked") && !line.includes("CGSSessionScreenIsLocked")) {
      continue;
    }
    if (line.includes("= Yes") || line.includes("=Yes")) {
      return true;
    }
    if (line.includes("= No") || line.includes("=No")) {
      return false;
    }
  }
  return null;
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: repoRoot,
      stdio: options.stdio || "inherit",
      env: { ...process.env, ...options.env },
    });
    child.on("error", reject);
    child.on("close", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} exited with code ${code ?? "null"} signal ${signal ?? "null"}`));
      }
    });
  });
}

function waitFor(child) {
  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`screencapture exited with code ${code ?? "null"} signal ${signal ?? "null"}`));
      }
    });
  });
}

function runSyncChecked(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: options.stdio || "pipe",
    env: { ...process.env, ...options.env },
  });
  if (result.status !== 0) {
    const stderr = (result.stderr || "").trim();
    throw new Error(`${command} failed with code ${result.status}: ${stderr}`);
  }
  return result.stdout || "";
}

function removeIfExists(file) {
  try {
    fs.rmSync(file, { force: true });
  } catch (error) {
    throw new Error(`Failed to remove stale artifact ${file}: ${error.message}`);
  }
}

function prepareSafariForRecording() {
  if (process.platform !== "darwin") {
    return false;
  }
  const script = [
    'tell application "Safari"',
    "  activate",
    "  if (count of windows) is 0 then make new document",
    "  set bounds of front window to {80, 60, 1141, 974}",
    "end tell",
    "delay 0.5",
  ].join("\n");
  runSyncChecked("osascript", ["-e", script]);
  return true;
}

function readRecipeResponse(outputFile) {
  const lines = fs
    .readFileSync(outputFile, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  return lines.find((entry) => entry.id === "run-flower-demo");
}

function structuredContent(entry) {
  return entry?.result?.structuredContent ?? null;
}

async function postprocessRecording(plan) {
  const response = readRecipeResponse(plan.responses);
  const content = structuredContent(response);
  if (!response || content?.success !== true) {
    const detail = response ? JSON.stringify(content, null, 2) : "missing recipe response";
    throw new Error(`Sootie recipe did not complete successfully. Response: ${detail}`);
  }
  const lastScreenshot = content?.data?.last_screenshot?.artifact_path;
  if (lastScreenshot && fs.existsSync(lastScreenshot)) {
    fs.copyFileSync(lastScreenshot, plan.final_image);
  } else {
    console.warn("Recipe completed but did not expose a copyable final screenshot artifact.");
  }

  if (plan.commands.ffmpeg) {
    if (!fs.existsSync(plan.raw_recording)) {
      throw new Error(`Raw recording does not exist: ${plan.raw_recording}`);
    }
    await run("ffmpeg", [
      "-y",
      "-i",
      plan.raw_recording,
      "-vf",
      "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2",
      "-r",
      "30",
      "-c:v",
      "libx264",
      "-pix_fmt",
      "yuv420p",
      "-movflags",
      "+faststart",
      plan.final_video,
    ]);
  } else if (plan.commands.avconvert) {
    if (!fs.existsSync(plan.raw_recording)) {
      throw new Error(`Raw recording does not exist: ${plan.raw_recording}`);
    }
    await run("avconvert", [
      "--source",
      plan.raw_recording,
      "--preset",
      "Preset1920x1080",
      "--output",
      plan.final_video,
      "--replace",
    ]);
  } else {
    console.warn(`ffmpeg/avconvert not found; keeping raw recording at ${plan.raw_recording}`);
  }
}

function resolveRecipe(args) {
  const recipeFileArg = argValue(
    args,
    "--recipe-file",
    "docs/development/recipes/safari-excalidraw-human-actions-red-flower.recipe.json",
  );
  const recipeFile = recipeFileArg === "none" ? null : resolveRepoPath(recipeFileArg);
  const recipeJson = recipeFile ? fs.readFileSync(recipeFile, "utf8") : null;
  const recipeName = argValue(
    args,
    "--recipe",
    recipeJson ? JSON.parse(recipeJson).name : "safari-excalidraw-human-actions-red-flower",
  );
  return { recipeName, recipeFile, recipeJson };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    return;
  }

  const { recipeName: recipe, recipeFile, recipeJson } = resolveRecipe(args);
  const server = resolveRepoPath(argValue(args, "--server", defaultServer()));
  const outputDir = resolveRepoPath(argValue(args, "--output-dir", "~/Desktop/sootie/demo"));
  const outputName = argValue(
    args,
    "--output",
    "sootie-excalidraw-human-actions-red-flower-fullscreen-demo.mp4",
  );
  const finalImageName = argValue(
    args,
    "--final-image",
    "sootie-excalidraw-human-actions-red-flower-final.png",
  );
  const duration = Number(argValue(args, "--duration", "45"));
  const display = argValue(args, "--display", "1");
  const dryRun = args.includes("--dry-run");
  const checkOnly = args.includes("--check-only");
  const skipWindowPrepare = args.includes("--skip-window-prepare");
  const postprocessOnly = args.includes("--postprocess-only");

  if (!Number.isFinite(duration) || duration <= 0) {
    throw new Error("--duration must be a positive number of seconds");
  }

  const locked = macosScreenLocked();
  const mp4Path = path.join(outputDir, outputName);
  const movPath = path.join(outputDir, outputName.replace(/\.mp4$/i, ".raw.mov"));
  const requestPath = path.join(os.tmpdir(), "sootie-excalidraw-flower-demo-request.jsonl");
  const responsePath = path.join(outputDir, `${path.basename(outputName, ".mp4")}.responses.jsonl`);
  const finalImagePath = path.join(outputDir, finalImageName);
  const plan = {
    recipe,
    recipe_file: recipeFile,
    server,
    output_dir: outputDir,
    raw_recording: movPath,
    final_video: mp4Path,
    final_image: finalImagePath,
    responses: responsePath,
    duration_seconds: duration,
    display,
    window_prepare: !skipWindowPrepare,
    screen_locked: locked,
    commands: {
      screencapture: commandExists("screencapture"),
      ffmpeg: commandExists("ffmpeg"),
      avconvert: commandExists("avconvert"),
    },
  };

  if (dryRun || checkOnly) {
    console.log(JSON.stringify(plan, null, 2));
  }
  if (locked === true) {
    throw new Error("macOS screen is locked; unlock the Mac before recording the fullscreen Sootie demo.");
  }
  if (!plan.commands.screencapture) {
    throw new Error("screencapture is required on macOS for fullscreen demo recording.");
  }
  if (!fs.existsSync(server)) {
    throw new Error(`Sootie server binary does not exist: ${server}`);
  }
  if (checkOnly || dryRun) {
    return;
  }
  fs.mkdirSync(outputDir, { recursive: true });
  if (postprocessOnly) {
    await postprocessRecording(plan);
    console.log(JSON.stringify({ ...plan, completed: true, postprocess_only: true }, null, 2));
    return;
  }
  for (const staleArtifact of [movPath, mp4Path, finalImagePath, responsePath]) {
    removeIfExists(staleArtifact);
  }

  const requests = [];
  if (recipeJson) {
    requests.push({
      jsonrpc: "2.0",
      id: "save-flower-demo-recipe",
      method: "tools/call",
      params: {
        name: "recipe_save",
        arguments: { recipe_json: recipeJson },
      },
    });
  }
  requests.push({
      jsonrpc: "2.0",
      id: "run-flower-demo",
      method: "tools/call",
      params: {
        name: "run",
        arguments: { recipe },
      },
    });
  fs.writeFileSync(requestPath, `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`);

  if (!skipWindowPrepare) {
    prepareSafariForRecording();
  }

  const capture = spawn(
    "screencapture",
    ["-x", "-v", `-V${duration}`, `-D${display}`, "-k", movPath],
    { cwd: repoRoot, stdio: "inherit" },
  );
  await new Promise((resolve) => setTimeout(resolve, 1000));
  let recipeError = null;
  try {
    await run("node", [
      "docs/development/run-jsonl-mcp-smoke.mjs",
      "--server",
      server,
      "--template",
      requestPath,
      "--output",
      responsePath,
      "--allow-placeholders",
      "--timeout-ms",
      String(Math.max(120000, Math.ceil(duration * 1000) + 60000)),
    ]);
  } catch (error) {
    recipeError = error;
  }
  let captureError = null;
  try {
    await waitFor(capture);
  } catch (error) {
    captureError = error;
  }
  if (captureError) {
    throw captureError;
  }
  await postprocessRecording(plan);
  if (recipeError) {
    throw recipeError;
  }

  console.log(JSON.stringify({ ...plan, completed: true }, null, 2));
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
