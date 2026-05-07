---
title: "Refactor Vision Sidecar Architecture (Optimized)"
type: refactor
status: active
date: 2026-05-07
---

# Refactor Vision Sidecar Architecture

## Overview

Extract the Python sidecar from inline Rust into independent files, add production-grade lifecycle management (lazy load, idle timeout, signal handling), harden model download integrity with SHA256 checksums, enhance security with shared secret authentication, and implement crash recovery with retry logic.

## Problem Frame

Current implementation has critical gaps:
- **Startup blocking**: Model loads at startup (~30s), no lazy loading
- **No lifecycle management**: No idle timeout, runs forever (~4GB RAM)
- **Incomplete integrity**: File size only, no checksum validation
- **No security**: HTTP localhost without authentication
- **No crash recovery**: Assumes sidecar stable, no retry logic
- **Python detection weak**: Accepts <3.10, no upper bound check
- **No venv versioning**: Stale deps persist across upgrades
- **Hand-rolled prompts**: No mature CLI library

## Requirements Trace

- R1. Python sidecar server must be an independent, testable, versionable Python file
- R2. Sidecar must support lazy model loading, idle timeout, signal handling, concurrent request handling
- R3. Model download must validate SHA256 checksum from HF API metadata
- R4. Python detection must validate 3.10-3.13 range (scipy wheel availability)
- R5. Venv must be stamped with sootie version and recreated when stale
- R6. HTTP endpoints must authenticate requests with shared secret
- R7. Sidecar launch must implement crash recovery with retry logic
- R8. Image input validation must enforce size/format limits before PIL processing
- R9. Interactive prompts should use `dialoguer` crate

## Scope Boundaries

- NOT adding YOLO element detection (placeholder only)
- NOT adding cloud VLM provider (still stub)
- NOT changing `VisionProvider` trait or `SidecarVisionProvider` in sootie-core
- NOT adding crop-based grounding (deferred)
- NOT adding Windows support (macOS + Linux only)

## Key Technical Decisions

### Architecture Simplification

**Decision: Direct Python invocation (no bash launcher)**
- **Rationale**: Single-user context, Rust can resolve Python path directly
- **Benefit**: Removes PATH dependency, shell layer, debugging complexity
- **Implementation**: Rust resolves venv python path, spawns Python directly

### Security Enhancement

**Decision: Shared secret authentication for HTTP endpoints**
- **Rationale**: Localhost-only insufficient for production
- **Benefit**: Prevents unauthorized local processes from abusing sidecar
- **Implementation**: `--auth-token` CLI arg, `X-Sootie-Auth` header validation

**Decision: SHA256 checksum validation for model downloads**
- **Rationale**: File size cannot detect corruption or substitution
- **Benefit**: Production-grade integrity verification
- **Implementation**: Fetch checksum from HF API, verify post-download

### Reliability Design

**Decision: Concurrent request handling with blocking during model load**
- **Rationale**: Simpler than 503 retry logic, load is one-time
- **Benefit**: Users get consistent behavior during initial load
- **Implementation**: `threading.Condition` blocks concurrent requests during load (max 30s timeout)

**Decision: Crash recovery with health check + retry (3x, exponential backoff)**
- **Rationale**: Sidecar crashes are inevitable (memory exhaustion, network timeout)
- **Benefit**: Graceful degradation, no silent failures
- **Implementation**: `launch_sidecar()` retries on health check failure, then returns Err

**Decision: Image input validation (50MB limit, JPEG/PNG only)**
- **Rationale**: Prevent memory exhaustion, PIL DoS vectors
- **Benefit**: Production-grade input sanitization
- **Implementation**: Check decoded size before PIL.open, validate format

### Data Directory First

**Decision: `~/.local/share/sootie/vision-sidecar/` as primary location**
- **Rationale**: No PATH manipulation burden on users
- **Benefit**: Guaranteed location, simpler deployment
- **Implementation**: `find_sidecar_script()` checks data dir first

## Component Architecture

```
~/.local/share/sootie/
  venv/                        # Python 3.10-3.13 venv
    .sootie-version            # Version stamp (crate version)
    bin/python3                # Venv Python
  models/ShowUI-2B/
    model.safetensors          # ~2.97GB
    .checksums                 # SHA256 checksums from HF API
  vision-sidecar/
    server.py                  # HTTP server with auth + lazy load
    requirements.txt           # Deps manifest
```

## Data Flow: Vision Request

```
Agent -> MCP -> SidecarVisionProvider
              | launch_sidecar(port, auth_token)
         Rust process spawn
              | Command::new(venv_python, server.py, --port, --auth-token)
         server.py (lazy load + auth validation)
              | POST /ground (X-Sootie-Auth header)
              | Model loads on first request (10-15s, concurrent requests block)
              | PIL image validation (size, format)
         MLX inference
              | ShowUI-2B -> [x,y]
```

## Data Flow: Setup

```
sootie setup
  |-- check.rs: 5 checks
  |-- fix "Vision model + sidecar":
  |     |-- python_env.rs: find_python (3.10-3.13), create_venv, install_deps
  |     |-- model_download.rs: fetch checksums, download, SHA256 verify
  |     |-- sidecar.rs: copy server.py to data dir
  +-- re-run checks -> final status
```

## Data Flow: Crash Recovery

```
launch_sidecar(port)
  +-- loop (max 3 retries):
        +-- spawn python process
        +-- wait_for_health(port, 10s)
        +-- if success: return SidecarGuard
        +-- if timeout: kill child, sleep (exponential backoff: 2s, 4s, 8s)
  +-- if all retries fail: return Err with last stderr lines
```

## Implementation Units

### Unit 1: Create `vision-sidecar/server.py` with auth + reliability

**Goal**: Production-grade HTTP server with security, lifecycle, and reliability features.

**Requirements**: R1, R2, R6, R8

**Files**:
- Create: `vision-sidecar/server.py`
- Create: `vision-sidecar/requirements.txt`

**Approach**:

Core features:
- **Argparse CLI**: `--port`, `--auth-token`, `--model-path`, `--idle-timeout`, `--health-check`, `--preload`, `--version`
- **Authentication**: `X-Sootie-Auth` header validation on all POST endpoints, reject 401 if missing/invalid
- **Lazy loading**: `_load_vlm()` with `threading.Lock` + `threading.Condition`, concurrent requests block during load (max 30s)
- **Idle timeout**: `threading.Timer` resets per request, 600s default, auto-exit
- **Signal handling**: `SIGTERM`/`SIGINT` -> cleanup temp files -> `server.shutdown()`
- **Health endpoint**: JSON `{"status", "status_detail", "version", "models_loaded", "model_path", "model_exists", "vlm_load_error", "idle_timeout", "pid"}`
  - `status`: "idle" | "loading" | "ready" | "error"
  - `status_detail`: human-readable state description
- **Input validation**: 
  - Max decoded image size: 50MB
  - `Image.MAX_IMAGE_PIXELS = 100_000_000` (prevent decompression bombs)
  - Format whitelist: JPEG, PNG only
  - Reject malformed images with 400
- **Model integrity**: Pre-check model.safetensors exists and >2.5GB before load attempt
- **Temp file cleanup**: `tempfile.NamedTemporaryFile(delete=True)` or `atexit` handler for tracked paths

Endpoints:
- `/health` GET -> JSON status
- `/ground` POST (auth required) -> coordinates
- `/detect` POST (auth required, placeholder)
- `/parse` POST (auth required, placeholder)

`requirements.txt`:
```
mlx>=0.21.0,<1.0.0
mlx-lm>=0.21.5,<0.30.0
transformers>=4.38.0,<4.49.0
numpy>=1.23.4
Pillow>=10.0.0,<12.0.0
```

**Test scenarios**:
- Happy path: `--health-check` with valid model -> exit 0
- Happy path: server starts, `/health` returns `{"status": "idle"}`
- Auth test: `/ground` without header -> 401
- Auth test: `/ground` with wrong token -> 401
- Concurrent test: 2 requests during model load -> both succeed after load completes
- Input validation: oversized image (>50MB) -> 400
- Input validation: non-image content -> 400
- Idle timeout: no requests for 600s -> process exits
- Signal test: SIGTERM during load -> graceful shutdown

---

### Unit 2: Create `python_env.rs` with version bounds

**Goal**: Python detection with 3.10-3.13 bounds, venv versioning, dep installation.

**Requirements**: R4, R5

**Files**:
- Create: `crates/sootie-cli/src/setup/python_env.rs`
- Modify: `crates/sootie-cli/src/setup/mod.rs`
- Modify: `crates/sootie-cli/src/setup/sidecar.rs` (remove Python logic)

**Approach**:

`find_python()`: Candidate path resolution:
```rust
let candidates = [
    "/opt/homebrew/bin/python3.13",
    "/opt/homebrew/bin/python3.12",
    "/opt/homebrew/bin/python3.11",
    "/opt/homebrew/bin/python3.10",
    "/usr/local/bin/python3.13",
    "/usr/local/bin/python3.12",
    "/usr/local/bin/python3.11",
    "/usr/local/bin/python3.10",
    "/usr/bin/python3.13",
    "/usr/bin/python3.12",
    "/usr/bin/python3.11",
    "/usr/bin/python3.10",
    "python3", // fallback
];

for path in candidates {
    let version = parse_version(path)?;
    if version >= (3, 10) && version <= (3, 13) {
        return Ok(path);
    }
}
return Err("Python 3.10-3.13 required for MLX + scipy wheel availability");
```

`create_venv()`: Version stamp logic:
```rust
let version_file = venv_dir.join(".sootie-version");
if version_file.exists() {
    let existing_version = fs::read_to_string(&version_file)?;
    if existing_version.trim() != crate_version {
        fs::remove_dir_all(&venv_dir)?;
        create_venv_internal(&venv_dir, python_path)?;
        fs::write(&version_file, crate_version)?;
    }
} else {
    // Legacy venv without stamp -> recreate
    if venv_dir.exists() {
        fs::remove_dir_all(&venv_dir)?;
    }
    create_venv_internal(&venv_dir, python_path)?;
    fs::write(&version_file, crate_version)?;
}
```

`install_deps(platform)`: macOS with `--no-deps` hack:
```rust
// macOS: --no-deps mlx-vlm + pinned transformers
cmd.args(["--no-deps", "mlx-vlm==0.1.15"]);
cmd.args(["install", "transformers==4.48.3", "mlx-lm>=0.21.5,<0.30.0", "mlx", "Pillow", "numpy>=1.23.4"]);

// Verify import
let verify = Command::new(venv_python)
    .args(["-c", "import mlx_vlm; print('OK')"])
    .output()?;
if !verify.status.success() {
    return Err("mlx-vlm import failed after install");
}
```

`check_python_deps()`: Moved from sidecar.rs, uses venv python.

Remove from `sidecar.rs`: all Python detection/venv/install functions.

**Test scenarios**:
- Happy path: Python 3.13 found -> Ok
- Edge case: Only Python 3.9 -> Err with install instructions
- Edge case: Python 3.14 -> skipped (upper bound)
- Edge case: venv with stale version -> recreated
- Error path: pip fails -> Err with last 5 lines

---

### Unit 3: Harden `model_download.rs` with SHA256 checksum

**Goal**: Production-grade integrity verification with checksum validation.

**Requirements**: R3

**Files**:
- Modify: `crates/sootie-cli/src/setup/model_download.rs`

**Approach**:

**Checksum retrieval**: Fetch SHA256 from HF API
```rust
#[derive(Deserialize)]
struct HfFileEntry {
    path: String,
    size: Option<u64>,
    sha256: Option<String>, // Add checksum field
}

fn list_files() -> Result<HashMap<String, (u64, String)>> {
    let entries: Vec<HfFileEntry> = resp.json()?;
    let mut file_info = HashMap::new();
    for entry in entries {
        if let (Some(size), Some(sha256)) = (entry.size, entry.sha256) {
            file_info.insert(entry.path, (size, sha256));
        }
    }
    Ok(file_info)
}
```

**Checksum validation**: Verify post-download
```rust
fn verify_checksum(path: &Path, expected_sha256: &str) -> Result<()> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let actual = hex::encode(hasher.finalize());
    if actual != expected_sha256 {
        fs::remove_file(path)?;
        return Err(format!("Checksum mismatch: expected {}, got {}", expected_sha256, actual));
    }
    Ok(())
}
```

**Selective cleanup**: Remove problematic files only, not entire directory
```rust
// If pytorch_model.bin exists, remove only .bin files
for entry in fs::read_dir(model_dir)? {
    if entry.path().extension() == Some("bin") {
        fs::remove_file(entry.path())?;
    }
}
// If model.safetensors exists but <2.5GB, remove only that file
let safetensors = model_dir.join("model.safetensors");
if safetensors.exists() && safetensors.metadata()?.len() < 2_500_000_000 {
    fs::remove_file(&safetensors)?;
}
```

**Size-aware download**:
```rust
fn download_file(url: &str, dest: &Path, expected_size: u64, expected_sha256: &str) -> Result<()> {
    // Check existing file
    if dest.exists() {
        let size = dest.metadata()?.len();
        if size == expected_size {
            // Size matches, verify checksum
            if verify_checksum(dest, expected_sha256).is_ok() {
                return Ok(()); // Skip download
            }
            // Checksum mismatch, delete and re-download
            fs::remove_file(dest)?;
        }
        if size == 0 {
            // Zero-length file, delete
            fs::remove_file(dest)?;
        }
    }
    
    // Download
    download_to_file(url, dest)?;
    
    // Verify
    verify_checksum(dest, expected_sha256)?;
    
    Ok(())
}
```

**Post-download validation**:
- model.safetensors must be >2.5GB (minimum viable size)
- All REQUIRED_FILES present
- Verify safetensors header magic bytes

**Test scenarios**:
- Happy path: download + checksum validation succeeds
- Edge case: existing file with wrong checksum -> deleted, re-downloaded
- Edge case: zero-length file -> deleted, re-downloaded
- Error path: checksum mismatch after download -> Err, file deleted
- Edge case: partial download (93MB) -> size mismatch triggers re-download

---

### Unit 4: Refactor `sidecar.rs` with direct Python spawn + crash recovery

**Goal**: Simplify launch logic, remove bash launcher, add crash recovery.

**Requirements**: R7

**Dependencies**: Unit 1, Unit 2

**Files**:
- Modify: `crates/sootie-cli/src/setup/sidecar.rs`

**Approach**:

**Remove bash launcher complexity**:
- Remove: `generate_sidecar_launcher()`, sidecar bash script installation
- Remove: PATH manipulation, ~/.local/bin/ dependency

**Direct Python invocation**:
```rust
fn launch_sidecar(port: u16, auth_token: &str) -> Result<SidecarGuard> {
    // Check if already running
    if is_sidecar_running(port)? {
        return Ok(SidecarGuard::empty());
    }
    
    // Resolve paths
    let python = sootie_venv_python()?; // From python_env.rs
    let script = sidecar_install_dir().join("server.py");
    
    // Generate auth token if not provided
    let token = if auth_token.is_empty() {
        generate_random_token()?
    } else {
        auth_token
    };
    
    // Retry loop with exponential backoff
    for attempt in 0..3 {
        let delay = 2u64.pow(attempt);
        if attempt > 0 {
            sleep(Duration::from_secs(delay));
        }
        
        // Spawn process
        let child = Command::new(&python)
            .arg(&script)
            .arg("--port")
            .arg(port.to_string())
            .arg("--auth-token")
            .arg(&token)
            .arg("--idle-timeout")
            .arg("600")
            .stderr(Stdio::piped()) // Capture stderr for error reporting
            .stdout(Stdio::null())
            .spawn()?;
        
        // Health check
        match wait_for_sidecar(port, 10)? {
            true => {
                // Store auth token for SidecarVisionProvider
                return Ok(SidecarGuard::new(child, token));
            }
            false => {
                // Kill and retry
                child.kill()?;
                let _ = child.wait();
                if attempt == 2 {
                    // Last attempt, return Err with stderr
                    let stderr = child.stderr?;
                    return Err(format!("Sidecar failed to start after 3 retries. Last stderr: {}", 
                        read_stderr_lines(stderr, 5)));
                }
            }
        }
    }
}
```

**Sidecar file copy**:
```rust
fn install_sidecar_files() -> Result<()> {
    let src_dir = find_bundled_sidecar_dir()?; // Check installed/dev paths
    let dest_dir = sidecar_install_dir(); // ~/.local/share/sootie/vision-sidecar/
    
    fs::create_dir_all(&dest_dir)?;
    fs::copy(src_dir.join("server.py"), dest_dir.join("server.py"))?;
    fs::copy(src_dir.join("requirements.txt"), dest_dir.join("requirements.txt"))?;
    
    Ok(())
}
```

**Crash detection in SidecarVisionProvider**:
```rust
// In sootie-core/src/vision/sidecar_provider.rs
impl SidecarVisionProvider {
    fn ground(&self, image: &str, prompt: &str) -> Result<Vec<f32>> {
        // Health check before request
        if !self.health_check()? {
            // Sidecar crashed, attempt restart
            self.launch_sidecar()?;
        }
        
        // POST with auth
        let response = self.client
            .post(&self.url)
            .header("X-Sootie-Auth", &self.auth_token)
            .json(&{"image": image, "prompt": prompt})
            .send()?;
        
        match response.status() {
            200 => Ok(response.json()?),
            401 => Err("Auth token rejected"),
            503 => Err("Model loading or error"),
            _ => Err("Unexpected status"),
        }
    }
}
```

**Keep in sidecar.rs**:
- `SidecarGuard::empty()` (already exists)
- `wait_for_sidecar()` (activate from `#[allow(dead_code)]`)
- `is_sidecar_running()`

**Test scenarios**:
- Happy path: sidecar spawns, health check succeeds within 10s
- Edge case: sidecar already running -> return empty guard
- Error path: first spawn fails -> retry with 2s delay
- Error path: all retries fail -> Err with stderr
- Integration: auth token matches, `/ground` succeeds

---

### Unit 5: Add `dialoguer` for interactive prompts

**Goal**: Replace hand-rolled prompts with mature CLI library.

**Requirements**: R9

**Files**:
- Modify: `crates/sootie-cli/Cargo.toml`
- Modify: `crates/sootie-cli/src/setup/mod.rs`

**Approach**:
```rust
// Cargo.toml
[dependencies]
dialoguer = "0.12"

// mod.rs
fn ask_yes_no(prompt: &str, default_yes: bool) -> Result<bool> {
    dialoguer::Confirm::new()
        .with_prompt(prompt)
        .default(default_yes)
        .interact()
        .map_err(|e| anyhow!("Prompt failed: {}", e))
}

// Shell-specific PATH instruction (if needed for other features)
fn get_shell_config_hint() -> &'static str {
    match std::env::var("SHELL").unwrap_or_default().as_str() {
        s if s.contains("bash") => "~/.bashrc or ~/.profile",
        s if s.contains("zsh") => "~/.zshrc",
        s if s.contains("fish") => "~/.config/fish/config.fish",
        _ => "your shell configuration file",
    }
}
```

**Test scenarios**:
- Happy path: prompts work in interactive terminal
- Edge case: non-interactive terminal -> dialoguer handles gracefully

## System-Wide Impact

- **Simplified deployment**: No PATH manipulation, no bash launcher, data directory is guaranteed location
- **Enhanced security**: Shared secret prevents unauthorized access, input validation prevents DoS
- **Improved reliability**: Crash recovery with retry, health checks before requests, checksum validation
- **Better UX**: Shell-specific guidance when needed, dialoguer for prompts
- **API contract**: `SidecarVisionProvider` now requires auth token in constructor, HTTP contract unchanged

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| transformers==4.48.3 pin fragile | Version check at install; if fails, fallback to 4.49+ with PyTorch |
| Checksum unavailable from HF API | Fall back to size validation + safetensors header check |
| Auth token generation complexity | Use simple hex-encoded random bytes (32 bytes) |
| Concurrent request timeout (30s) | Document behavior, make timeout configurable via `--load-timeout` |
| Crash recovery overhead | Retry only 3x, then fail fast with clear error |
| scipy wheel unavailable on 3.14 | Upper bound check rejects >3.13 before install |
| `.sootie-version` unreadable | Treat as mismatch, recreate venv |
| Temp file accumulation | Use `delete=True` or `atexit` cleanup handler |

## Verification Checklist

- ✅ Sidecar starts <1s (lazy load)
- ✅ Idle timeout fires after 600s
- ✅ Auth token required for POST requests
- ✅ SHA256 checksum validated for model
- ✅ Crash recovery retries 3x with backoff
- ✅ Python 3.10-3.13 validated
- ✅ Image input >50MB rejected
- ✅ Concurrent requests handled during load
- ✅ Health check returns detailed status
- ✅ Dialoguer prompts work