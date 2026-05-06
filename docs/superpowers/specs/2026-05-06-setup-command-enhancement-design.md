# Setup Command Enhancement Design

## Problem

The current `sootie setup` command is a pure text display tool. It prints platform-specific
permission instructions and MCP configuration examples, but performs no actual validation,
generates no configuration files, and provides no model setup. Users must manually configure
environment variables and download models.

## Goal

Transform `sootie setup` into an actionable setup wizard that:
1. Actually validates accessibility permissions (not just prints instructions)
2. Checks Chrome CDP availability
3. Checks/downloads vision model from Hugging Face
4. Generates a TOML config file with sensible defaults
5. Reports environment variable state

## Design: Two-Phase Model (Brew Doctor Pattern)

### Phase 1: Check

Run all 5 checks, print a structured report with ✓/✗/⚠ status indicators.

### Phase 2: Fix (interactive or automatic)

For each failed check that is fixable, offer to fix it. The user can accept, skip, or
use `--fix` to auto-fix everything.

### CLI Interface

```
sootie setup           # Two-phase: check + interactive fix
sootie setup --check   # Phase 1 only: check and report
sootie setup --fix     # Auto-fix all fixable issues (non-interactive)
```

## Checks Specification

### [1/5] Accessibility Permissions

| Platform | Check Method | Fixable |
|----------|-------------|---------|
| macOS | Call `AXIsProcessTrusted()` via CoreFoundation | No — prompt user to System Settings |
| Linux | Check `AT_SPI_BUS_ADDRESS` env or `/usr/lib/at-spi2` existence | No — prompt to install at-spi2-core |
| Windows | Check UI Automation availability | No — informational |

### [2/5] Chrome CDP

HTTP GET to `http://{cdp_host}:{cdp_port}/json/version`.

- Success → ✓
- Connection refused → ✗, print Chrome launch command

Fixable: No (cannot auto-start Chrome), but prints the exact command.

### [3/5] Vision Model + Sidecar

**Architecture**: Python sidecar process (same pattern as Ghost OS). The Rust main
process communicates with a Python HTTP sidecar that runs ShowUI-2B via MLX (Apple
Silicon Metal GPU) or transformers (other platforms).

```
Rust (sootie serve) ──HTTP──→ Python sidecar (localhost:9876) ──MLX/transformers──→ ShowUI-2B
```

Check order:
1. `SOOTIE_VISION_MODEL_PATH` env var → validate directory exists with required files
2. `config.toml` `vision.model_path` → validate directory exists with required files
3. Neither → ⚠ (optional feature)

Required model files in the directory:
- `model.safetensors` (or sharded `model-00001-of-0000X.safetensors`)
- `config.json`
- `tokenizer.json`, `tokenizer_config.json`
- `preprocessor_config.json`

Additionally check Python sidecar dependencies:
4. Python 3 with `mlx-vlm` (macOS) or `transformers` + `torch` (other platforms)
5. Sidecar can start and respond to `/health`

Fixable: Yes. Interactive download from Hugging Face + Python dependency setup.

**Model download:**
- Source: `https://huggingface.co/mlx-community/ShowUI-2B-bf16-8bit` (macOS, 8-bit quantized, ~3GB)
- Alternative source: `https://huggingface.co/showlab/ShowUI-2B` (full bf16, other platforms)
- Target: `~/.local/share/sootie/models/ShowUI-2B/`
- Chinese mirror: `https://hf-mirror.com/mlx-community/ShowUI-2B-bf16-8bit`
- File filter: `*.safetensors`, `*.json`, `merges.txt`, `vocab.txt`, `vocab.json`, `tokenizer.model`
- **Reject** `pytorch_model.bin` if found (incompatible format)

**Python sidecar setup:**
- macOS: `pip install --no-deps "mlx-vlm==0.1.15" && pip install "transformers<4.49" "mlx-lm>=0.21.5" mlx Pillow numpy`
- Other: `pip install "transformers<4.49" torch Pillow numpy`
- Critical: `transformers>=4.49` imports `Qwen2VLVideoProcessor` which requires PyTorch
  tensors not available in MLX. Must pin `<4.49` on macOS.

After download, write `vision.model_path` to config.toml and `vision.sidecar_port` if
non-default.

### [4/5] Configuration File

Check: `dirs_next::config_dir().join("sootie/config.toml")` exists.

Fixable: Yes. Generate default config.toml.

Default content:
```toml
[vision]
model_path = ""
model_name = "showui-2b"
sidecar_port = 9876
use_gpu = false

[cdp]
host = "127.0.0.1"
port = 9222

[logging]
level = "info"
sanitize_logs = true
```

If file already exists, do not overwrite — just report ✓.

### [5/5] Environment Variables

Display all `SOOTIE_*` env vars with their current values (or default if unset).
Informational only, not fixable.

Variables:
- `SOOTIE_CDP_HOST` (default: 127.0.0.1)
- `SOOTIE_CDP_PORT` (default: 9222)
- `SOOTIE_CDP_WS_URL` (default: none)
- `SOOTIE_VISION_MODEL_PATH` (default: none)
- `SOOTIE_VISION_USE_GPU` (default: false)
- `SOOTIE_SENSITIVE_FIELDS` (default: none)

## Data Structures

```rust
enum CheckStatus {
    Pass,        // ✓
    Fail,        // ✗
    Warn,        // ⚠
}

struct CheckResult {
    name: &'static str,
    status: CheckStatus,
    message: String,
    fixable: bool,
}

enum SetupMode {
    Interactive,
    CheckOnly,
    AutoFix,
}
```

## Code Structure

### New Files

```
crates/sootie-cli/src/
  main.rs                # Modified: add --check/--fix args, call setup::run_setup()
  setup/
    mod.rs               # run_setup() orchestration + report printing
    check.rs             # Check functions + CheckResult/CheckStatus
    config.rs            # TOML config file read/write
    model_download.rs    # Hugging Face model download with progress bar
    sidecar.rs           # Python sidecar health check + dependency setup
```

### Dependency Changes (sootie-cli/Cargo.toml)

- `toml = "0.8"` — config file parsing
- `reqwest = { version = "0.12", features = ["json"] }` — CDP check + HF download (sootie-core already depends on reqwest)
- `indicatif = "0.17"` — download progress bar

### macOS Permission Check

Reuse `sootie_core::platform::macos::ax_fns::is_process_trusted()` which already
wraps `AXIsProcessTrusted()` via CoreFoundation FFI. No additional crates needed.
Gated by `#[cfg(target_os = "macos")]` in `check.rs`.

## Config Precedence at Runtime

Environment variables > config.toml > built-in defaults.

Setup only writes to config.toml; it does not set environment variables.

## Sample Output

```
$ sootie setup

Sootie Setup
============

[1/5] Accessibility permissions...   ✓ Granted
[2/5] Chrome CDP...                  ✗ Not detected
[3/5] Vision model + sidecar...      ⚠ Not configured (optional)
[4/5] Configuration file...          ✗ Not found
[5/5] Environment variables...       ⚠ Using defaults

2 issues found, 1 optional feature available.

Fix issues? [Y/n] y

[2/5] Chrome CDP: Start Chrome with remote debugging?
  /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
    --remote-debugging-port=9222 --user-data-dir=/tmp/chrome-debug
  Skip for now? [Y/n] y

[4/5] Config: Generate config.toml? [Y/n] y
  ✓ Created ~/Library/Application Support/sootie/config.toml

[3/5] Vision: Download ShowUI-2B model (~3GB from Hugging Face)? [y/N] y
  Downloading model... ████████████████████ 100%
  ✓ Model saved to ~/.local/share/sootie/models/ShowUI-2B/
  Installing Python dependencies (mlx-vlm, transformers)...
  ✓ Python sidecar ready

Setup complete!
✓ Layer 1 (CDP): Available when Chrome starts with --remote-debugging-port
✓ Layer 2 (AT Tree): Available (Accessibility API granted)
✓ Layer 3 (Vision): ShowUI-2B via Python sidecar on port 9876
```

## Edge Cases

- Config directory doesn't exist → create it before writing config.toml
- Model download interrupted → delete partial file, report error
- Network unavailable for CDP/HF checks → ✗ with clear "network error" message
- Config.toml exists but malformed → ⚠, suggest manual fix, don't overwrite
- Hugging Face download 404 → ✗ with model URL for manual download
- Chinese mirror fallback → try hf-mirror.com if huggingface.co fails
- `pytorch_model.bin` found in model dir → ✗, prompt user to delete and re-download
- Python not found → ⚠, prompt to install Python 3.10+
- `transformers>=4.49` installed on macOS → ⚠, warn about MLX incompatibility
- Sidecar port already in use → ⚠, suggest changing `sidecar_port` in config.toml
