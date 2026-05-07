---
title: "Refactor Vision Sidecar Architecture"
type: refactor
status: active
date: 2026-05-07
---

# Refactor Vision Sidecar Architecture

## Overview

Extract the Python sidecar from an inline Rust string into a first-class directory, add production-grade lifecycle management (lazy load, idle timeout, signal handling, argparse), harden model download integrity, rewrite Python environment detection to match ghost-os proven patterns, and introduce `dialoguer` for interactive prompts.

## Problem Frame

The vision sidecar is a toy embedded in a Rust string literal. It lacks lazy loading (blocks startup ~30s), idle timeout, signal handling, and proper CLI args. Model downloads silently accept incomplete files. Python detection doesn't reject <3.10. The venv has no version stamp, so stale deps persist across upgrades. Interactive prompts are hand-rolled.

## Requirements Trace

- R1. Python sidecar server must be an independent, testable, versionable Python file (not inline Rust)
- R2. Sidecar must support lazy model loading, idle timeout, signal handling, and CLI args
- R3. Model download must detect and remove incomplete/corrupt files before and after download
- R4. Python detection must reject Python <3.10 and prefer versioned paths over bare `python3`
- R5. Venv must be stamped with sootie version and recreated when stale
- R6. Interactive prompts should use a mature CLI library (dialoguer)
- R7. Sidecar launch should use a bash launcher script (decoupled from Python path resolution)
- R8. Model download must validate file sizes against HF API metadata
- R9. Sidecar launch must check if already running before spawning (no duplicate processes)

## Scope Boundaries

- NOT adding YOLO element detection (placeholder only)
- NOT adding cloud VLM provider (still stub)
- NOT changing the `VisionProvider` trait or `SidecarVisionProvider` in sootie-core
- NOT adding crop-based grounding in this iteration (deferred)
- NOT adding Windows support for the sidecar (macOS + Linux only for now)

## Context & Research

### Relevant Code and Patterns

- **ghost-os `SetupWizard.swift`**: Proven Python venv management with version-stamped rebuilds, `--no-deps` mlx-vlm install, Python >=3.10 gate, model file size validation
- **ghost-os `vision-sidecar/server.py`**: Reference implementation with lazy load, idle timeout, argparse, signal handling, health endpoint, transformers version check
- **ghost-os `vision-sidecar/ghost-vision`**: Bash launcher resolving Python (venv > system, with `import mlx_vlm` verification)
- **Current `sidecar.rs`**: Python detection, venv creation, inline sidecar script (~120 lines), `SidecarGuard`, `launch_sidecar`
- **Current `model_download.rs`**: HF API file listing, streaming download with progress bar, no integrity check beyond file existence
- **`dialoguer` crate 0.12**: `Confirm`, `Select`, `MultiSelect` -- same ecosystem as `indicatif`

### Key Gaps

1. `download_file()` skips existing files regardless of size -- 93MB partial treated as complete
2. `find_python()` accepts any Python 3.x -- 3.9 passes but MLX requires >=3.10
3. No venv version stamp -- upgrading sootie won't rebuild venv
4. `install_macos_deps` installs `mlx-vlm` without `--no-deps` -- pip pulls PyTorch
5. Sidecar loads model at startup (30s blocking) -- no lazy load
6. No idle timeout -- sidecar runs forever (~4GB RAM)
7. Sidecar stdout/stderr set to null -- no logs for debugging
8. No check if sidecar already running -- multiple `sootie serve` spawn duplicate processes
9. Bundled vision-sidecar files location undefined -- "copy from repo" without path resolution
10. ~/.local/bin/ creation/PATH check missing -- launcher install incomplete

## Key Technical Decisions

- **Extract server.py to `vision-sidecar/`**: Independent testing, linting, iteration. Rust copies to `~/.local/share/sootie/vision-sidecar/` at setup time.
- **`--no-deps` mlx-vlm + pinned transformers==4.48.3**: mlx-vlm 0.1.15 metadata says `transformers>=4.49.0` but works with 4.48.3. Without `--no-deps`, pip pulls PyTorch. Ghost-os approach.
- **Bash launcher `sootie-vision`**: Decouples Python path resolution from Rust. `launch_sidecar` calls bash, bash finds Python. Ghost-os pattern.
- **Python >=3.10 minimum, prefer 3.13**: MLX requires >=3.10. Python 3.13 has best scipy/numpy wheel availability.
- **Venv version stamp**: Write `.sootie-version` with crate version. Mismatch = `rm -rf` + recreate.
- **File size validation from HF API**: Store expected sizes during `list_files()`. Post-download verify. Mismatch = delete + re-download.
- **Lazy model loading**: Model loads on first `/ground` request. Startup <1s. Health reports `idle` vs `ready`.
- **Idle timeout 600s**: Auto-exit after 10 min idle. Configurable via `--idle-timeout`.
- **`dialoguer` for prompts**: Replace `ask_yes_no` with `Confirm`, add `Select` for future model choice.

## Open Questions

### Resolved During Planning

- **Q: Embed server.py at compile time or copy at setup time?** A: Copy at setup time. Allows updating sidecar without recompiling sootie.
- **Q: Bash launcher system-wide or in data dir?** A: Install to `~/.local/bin/sootie-vision` (on PATH) for debugging. Fallback copy in `~/.local/share/sootie/vision-sidecar/sootie-vision`.

### Deferred to Implementation

- Exact `dialoguer` prompt wording: UX iteration during implementation
- Whether to add `--preload` flag: Useful for setup verification but not required

## High-Level Technical Design

### Component Architecture

```
~/.local/share/sootie/
  venv/                    # Python 3.13+ venv
    .sootie-version        # Version stamp
    bin/python3            # Venv Python
  models/ShowUI-2B/        # Model files
    model.safetensors      # ~2.97GB
  vision-sidecar/          # Copied from repo at setup
    server.py              # HTTP server
    sootie-vision          # Bash launcher
    requirements.txt       # Deps manifest
```

### Data Flow: Vision Request

```
Agent -> MCP -> SidecarVisionProvider
                  | HTTP POST /ground
             sootie-vision (bash)
                  | exec python3
             server.py (lazy model)
                  | MLX
             ShowUI-2B -> [x,y]
```

### Data Flow: Setup

```
sootie setup
  |-- check.rs: 5 checks
  |-- fix "Vision model + sidecar":
  |     |-- 1. python_env.rs: find_python >=3.10, create_venv, install_deps
  |     |-- 2. model_download.rs: rm incomplete, download, validate size
  |     |-- 3. copy vision-sidecar/ to data dir
  |     +-- 4. install sootie-vision to ~/.local/bin/
  +-- re-run checks -> final status
```

### Data Flow: Sidecar Launch

```
sootie serve
  +-- launch_sidecar(port)
        +-- Command::new("sootie-vision") --idle-timeout 600
              +-- bash: find python (venv > system)
                    +-- exec python3 server.py --port {port} --idle-timeout 600
                          +-- idle: /health returns {"status": "idle"}
                          +-- first /ground: model loads (10-15s)
                          +-- subsequent: /ground returns in 0.5-3s
                          +-- 600s idle: auto-exit
```

## Implementation Units

- [ ] **Unit 1: Create `vision-sidecar/` directory**

**Goal:** Extract Python sidecar from inline Rust string into independent files.

**Requirements:** R1, R2

**Dependencies:** None

**Files:**
- Create: `vision-sidecar/server.py`
- Create: `vision-sidecar/sootie-vision`
- Create: `vision-sidecar/requirements.txt`

**Approach:**

Port inline Python to `server.py` with ghost-os capabilities:
- `argparse` CLI: `--port`, `--model-path`, `--idle-timeout`, `--health-check`, `--preload`, `--version`
- Lazy model loading: `_load_vlm()` with `threading.Lock`, loads on first `/ground`
- Idle timeout: `threading.Timer` resets per request, 600s default, auto-exit
- Signal handling: `SIGTERM`/`SIGINT` -> `server.shutdown()` in background thread
- Health endpoint: JSON `{"status", "version", "models_loaded", "model_path", "model_exists", "vlm_load_error", "idle_timeout", "pid"}`
- Model path resolution: `--model-path` > env `SOOTIE_VISION_MODEL_PATH` > default `~/.local/share/sootie/models/ShowUI-2B`
- transformers version check: warn if >=4.49
- Model pre-check: warn if `model.safetensors` missing or too small
- `/ground`, `/detect` (placeholder), `/parse` (placeholder) endpoints

`sootie-vision` bash launcher:
- Find Python: venv python (verify `import mlx_vlm`) -> system candidates (verify) -> PATH (verify) -> fail with instructions
- `exec "$PYTHON" "$SERVER_SCRIPT" "$@"`

`requirements.txt`: `mlx>=0.21.0,<1.0.0`, `mlx-lm>=0.21.5,<0.30.0`, `transformers>=4.38.0,<4.49.0`, `numpy>=1.23.4`, `Pillow>=10.0.0,<12.0.0`

**Patterns to follow:**
- ghost-os `vision-sidecar/server.py`
- ghost-os `vision-sidecar/ghost-vision`

**Test scenarios:**
- Happy path: `python3 server.py --health-check` with valid model -> exit 0
- Happy path: `python3 server.py --port 9877` -> `/health` returns `{"status": "idle"}`
- Edge case: no model dir -> `/health` returns `{"status": "idle", "model_exists": false}`
- Edge case: idle timeout fires -> process exits
- Error path: `--health-check` with corrupt model -> exit non-zero
- Error path: `/ground` with model load failure -> 503 with error detail

**Verification:**
- `python3 vision-sidecar/server.py --health-check` works standalone
- `sootie-vision` launcher resolves venv python
- Endpoints match `SidecarVisionProvider` protocol

---

- [ ] **Unit 2: Create `python_env.rs`**

**Goal:** Replace scattered Python logic in `sidecar.rs` with focused module matching ghost-os patterns.

**Requirements:** R4, R5

**Dependencies:** None

**Files:**
- Create: `crates/sootie-cli/src/setup/python_env.rs`
- Modify: `crates/sootie-cli/src/setup/mod.rs` (add `mod python_env;`, update exports)
- Modify: `crates/sootie-cli/src/setup/sidecar.rs` (remove Python detection/venv/install functions)

**Approach:**

`find_python()`: versioned candidate path list (`/opt/homebrew/bin/python3.13`, `3.12`, `3.11`, `3.10`, then `python3`, `/usr/local/bin/python3`, `/usr/bin/python3`). Parse `sys.version_info`, reject <3.10, also reject >3.13 (scipy wheel availability).

`create_venv()`: version stamp at `.sootie-version`. Mismatch = delete + recreate. Legacy venv (no stamp) = recreate. Write crate version after creation.

`install_deps(platform)`: macOS = `--no-deps mlx-vlm==0.1.15` + `transformers==4.48.3 mlx-lm>=0.21.5,<0.30.0 mlx Pillow numpy>=1.23.4` + verify import. Linux = `transformers<4.49 torch Pillow numpy` + verify.

`check_python_deps(platform)`: moved from sidecar.rs, uses venv python.

Remove from `sidecar.rs`: `find_python`, `find_python_313`, `python_version`, `check_mlx_vlm_installed`, `check_transformers_version`, `sootie_venv_path`, `sootie_venv_python`, `create_venv_if_needed`, `install_macos_deps`, `install_other_deps`, `fix_python_deps`, `check_python_deps`.

**Test scenarios:**
- Happy path: `find_python()` returns first >=3.10 Python
- Edge case: only Python 3.9 -> Err with install instructions
- Edge case: Python 2.7 -> skipped
- Happy path: `create_venv()` writes `.sootie-version`
- Edge case: venv with wrong version -> deleted and recreated
- Happy path: `install_deps("macos")` with `--no-deps` mlx-vlm
- Error path: pip fails -> Err with last 5 lines
- Integration: venv python can `import mlx_vlm` after install

**Verification:**
- `cargo test -p sootie-cli` passes
- `rm -rf ~/.local/share/sootie/venv && sootie setup` creates venv with >=3.10
- `.sootie-version` exists in venv

---

- [ ] **Unit 3: Harden `model_download.rs`**

**Goal:** Prevent incomplete/corrupt model files from being accepted.

**Requirements:** R3, R8

**Dependencies:** None

**Files:**
- Modify: `crates/sootie-cli/src/setup/model_download.rs`

**Approach:**

1. Store `HfFileEntry.size` from API in a `HashMap<String, u64>` during `list_files()`
2. Pre-download cleanup in `download_showui_model()`:
   - `pytorch_model.bin` exists without `model.safetensors` -> `rm -rf` (wrong format)
   - `validate_model_dir()` fails -> `rm -rf` (incomplete download)
3. Size-aware skip in `download_file()`:
   - File exists + size matches expected -> skip
   - File exists + size mismatch -> delete and re-download
   - File exists + no expected size -> skip (conservative for small files)
4. Post-download validation:
   - `model.safetensors` must be >1GB (like ghost-os check)
   - All REQUIRED_FILES present
5. Update `validate_model_dir()` to also check `model.safetensors` file size >1GB

**Test scenarios:**
- Happy path: download fresh model, validate size
- Edge case: existing partial file (93MB of 2.97GB) -> deleted, re-downloaded
- Edge case: pytorch_model.bin present -> entire dir removed
- Edge case: model.safetensors too small (<1GB) -> validation fails with clear error
- Error path: download interrupted -> next run detects incomplete, cleans up

**Verification:**
- `rm -rf ~/.local/share/sootie/models/ShowUI-2B && sootie setup` downloads complete model
- Model >1GB validated successfully

---

- [ ] **Unit 4: Refactor `sidecar.rs` -- slim down to launch + copy + guard**

**Goal:** Remove Python logic (moved to `python_env.rs`), remove inline Python script (moved to `vision-sidecar/`), add sidecar file copy logic, update launch to use bash launcher.

**Requirements:** R1, R7, R9

**Dependencies:** Unit 1, Unit 2

**Files:**
- Modify: `crates/sootie-cli/src/setup/sidecar.rs`
- Modify: `crates/sootie-cli/src/setup/mod.rs` (update exports)

**Approach:**

Remove from `sidecar.rs`:
- All Python detection/venv/install functions (moved to `python_env.rs`)
- `generate_sidecar_launcher()` with inline Python string
- `sidecar_script_path()`
- `SidecarChecker` (unused, `#[allow(dead_code)]`)

Add to `sidecar.rs`:
- `find_bundled_sidecar_dir()` -> locate vision-sidecar files in installed or development environment:
  - Installed: check `/opt/homebrew/share/sootie/vision-sidecar/`, `/usr/local/share/sootie/vision-sidecar/`
  - Development: `current_exe().parent().parent().parent()` (`.build/release/sootie` -> project root) + `vision-sidecar/`
  - Verify `server.py` exists in found dir
- `sidecar_install_dir()` -> `~/.local/share/sootie/vision-sidecar/`
- `install_sidecar_files()` -> copy from `find_bundled_sidecar_dir()` to `sidecar_install_dir()`:
  - Files: `server.py`, `sootie-vision`, `requirements.txt`
  - Also install `sootie-vision` to `~/.local/bin/sootie-vision`:
    - Create `~/.local/bin/` if it doesn't exist
    - Copy with `fs::copy()`, set permissions 0o755
    - Check if `~/.local/bin/` is on PATH; if not, print warning: `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc`
- `find_sidecar_launcher()` -> find `sootie-vision` in `~/.local/bin/` first, then `sidecar_install_dir()`
- `is_sidecar_running(port)` -> `GET http://127.0.0.1:{port}/health`, returns true if 200 OK
- `launch_sidecar(port)`:
  1. If `is_sidecar_running(port)` -> return `SidecarGuard::empty()` (no child to manage)
  2. Else: `Command::new(find_sidecar_launcher())` with args `["--port", "{port}", "--idle-timeout", "600"]`
  3. Stderr to inherit (for logs), stdout to null
  4. Spawn child, then call `wait_for_sidecar(port, 10)` to block until health check succeeds
  5. Return `SidecarGuard::new(child)` if healthy, Err if timeout

Keep in `sidecar.rs`:
- `SidecarGuard` (unchanged, `empty()` constructor already exists at sidecar.rs:406-408)
- `wait_for_sidecar()` (currently marked `#[allow(dead_code)]`, will be activated and used in `launch_sidecar`)

**Test scenarios:**
- Happy path: `find_bundled_sidecar_dir()` finds installed or dev location
- Edge case: `find_bundled_sidecar_dir()` finds nothing -> Err with install instructions
- Happy path: `install_sidecar_files()` copies all 3 files + installs launcher to ~/.local/bin/
- Edge case: ~/.local/bin/ doesn't exist -> created automatically
- Edge case: ~/.local/bin/ not on PATH -> warning printed with fix instructions
- Happy path: `launch_sidecar(9876)` when not running -> spawns `sootie-vision`, waits for health, returns guard
- Edge case: `launch_sidecar(9876)` when already running -> returns `SidecarGuard::empty()` without spawning
- Integration: sidecar starts, `/health` returns 200 within 10s
- Error path: `sootie-vision` not found -> Err with setup instructions
- Error path: sidecar spawns but doesn't respond to health check in 10s -> Err

**Verification:**
- `sootie setup` copies vision-sidecar files and installs launcher to ~/.local/bin/
- `sootie serve` starts sidecar via `sootie-vision`, health check passes within 10s
- Second `sootie serve` (while sidecar already running) doesn't spawn duplicate process
- `~/.local/bin/sootie-vision` exists and is executable

---

- [ ] **Unit 5: Add `dialoguer` dependency and replace `ask_yes_no`**

**Goal:** Replace hand-rolled interactive prompts with `dialoguer`.

**Requirements:** R6

**Dependencies:** None

**Files:**
- Modify: `crates/sootie-cli/Cargo.toml` (add `dialoguer = "0.12"`)
- Modify: `crates/sootie-cli/src/setup/mod.rs` (replace `ask_yes_no` with `dialoguer::Confirm`)

**Approach:**

Add `dialoguer = "0.12"` to `Cargo.toml`.

Replace `ask_yes_no(prompt, default_yes)` with:
```rust
dialoguer::Confirm::new()
    .with_prompt(prompt)
    .default(default_yes)
    .interact()?;
```

Remove the `ask_yes_no` function.

**Test scenarios:**
- Happy path: prompts display and accept user input (manual verification)
- Edge case: non-interactive terminal -> dialoguer handles gracefully

**Verification:**
- `cargo build -p sootie-cli` succeeds with new dep
- `sootie setup` prompts work with dialoguer

## System-Wide Impact

- **Interaction graph:** `check_vision_model()` (check.rs) calls `check_python_deps()` (now in python_env.rs). `fix_issue()` (mod.rs) calls `fix_python_deps()`, `download_showui_model()`, and `install_sidecar_files()`. `launch_sidecar()` (main.rs) calls updated `launch_sidecar()` (sidecar.rs) which now uses bash launcher.
- **Error propagation:** Failed venv creation -> clear message with pip install manual instructions. Failed model download -> mirror fallback, then manual instructions. Sidecar crash -> `SidecarGuard` kills on Drop, `launch_sidecar` waits for health before returning.
- **State lifecycle risks:** Venv dir cleanup (`rm -rf`) is destructive but recoverable (rebuild at next setup). Model dir cleanup similarly recoverable (re-download). Sidecar PID tracking for cleanup.
- **API surface parity:** `SidecarVisionProvider` HTTP contract unchanged. Launcher interface changed (bash script instead of direct Python) but transparent to callers.
- **Integration coverage:** E2E test: `sootie setup` -> `sootie serve` -> `/health` -> `/ground` -> coordinates returned.
- **Unchanged invariants:** Config file format, model path conventions, MCP tools, fallback priority, cascade behavior unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| scipy build failure on Python 3.14+ | Reject Python >3.13 or warn with fallback install instructions |
| `hf-mirror.com` unavailable | Fallback to primary `huggingface.co` as first attempt |
| `sootie-vision` not on PATH after install | Print warning with fix: `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc` |
| transformers <4.49 constraint too restrictive | Version check at install time; if 4.49+ is already installed, warn but don't block |
| dialoguer terminal compatibility issues | Handle `IOError` gracefully with fallback to non-interactive mode |
| Multiple sidecar processes spawned | `launch_sidecar()` checks `is_sidecar_running()` before spawning; if running, returns empty guard |
| Bundled sidecar files not found in dev | `find_bundled_sidecar_dir()` uses `current_exe()` path resolution; if not found, Err with build instructions |
| ~/.local/bin/ doesn't exist | Create directory automatically in `install_sidecar_files()` with `fs::create_dir_all()` |

## Sources & References

- ghost-os `vision-sidecar/`: `/Users/bytedance/git/ghost-os/vision-sidecar/server.py`, `ghost-vision`, `requirements.txt`
- ghost-os `SetupWizard.swift`: `/Users/bytedance/git/ghost-os/Sources/ghost/SetupWizard.swift`
- Current implementation: `crates/sootie-cli/src/setup/sidecar.rs`, `model_download.rs`, `mod.rs`
- dialoguer crate: https://crates.io/crates/dialoguer
