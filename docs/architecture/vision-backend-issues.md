---
title: Vision Backend Known Issues
type: architecture
status: active
date: 2026-05-08
---

# Vision Backend Known Issues

This document tracks discovered issues in the Vision backend and their resolution status.

## Issue 1: Auth Token Missing in Vision Requests ✅ FIXED

**Symptom**: `Sidecar returned 401: Missing X-Sootie-Auth header`

**Root Cause**: 
- Sidecar server generates auth token at startup
- `SidecarVisionProvider` had no mechanism to receive token
- HTTP requests lacked `X-Sootie-Auth` header

**Fix**: 
- Added `auth_token` field to `SidecarVisionProvider`
- Added `with_auth_token()` method
- `RuntimeVisionProvider::from_env()` reads token from `SOOTIE_SIDECAR_AUTH_TOKEN`
- `launch_sidecar()` returns token in `SidecarGuard`
- `run_serve()` sets environment variable

**Commit**: `0e99552 feat(vision): add auth token support for vision sidecar`

## Issue 2: Sidecar Process Lifecycle ✅ FIXED

**Symptom**: `Sidecar unreachable` after stdin closes

**Root Cause**: 
- `SidecarGuard::drop()` called `child.kill()`
- In MCP stdio mode, stdin closure triggers drop
- Sidecar process killed before Vision backend can use it

**Fix**: 
- Modified `SidecarGuard::drop()` to not kill child process
- Sidecar runs independently with idle timeout
- Process auto-exits after `idle_timeout` seconds (600s by default)

**Commit**: `0c0cf07 fix(vision): prevent SidecarGuard from killing sidecar process`

## Issue 3: Sidecar 503 Errors ⚠️ OPEN

**Symptom**: `Sidecar returned 503` during inference requests

**Manifestations**: 
1. `"Server running, model not loaded"` in health check
2. Inference requests return 503
3. Sidecar process exits/crashes during inference

**Possible Root Causes**: 
1. **Lazy model loading**: ShowUI-2B model (2.97GB) loads on first inference
   - Loading may timeout
   - Loading may fail silently
2. **Model path issues**: Model exists but not loadable
3. **Dependency issues**: Missing torch/PIL in sidecar environment
4. **Memory issues**: Model loading consumes excessive memory

**Evidence**: 
```json
{
  "status": "idle",
  "status_detail": "Server running, model not loaded",
  "models_loaded": false,
  "model_path": "/Users/bytedance/.local/share/sootie/models/ShowUI-2B",
  "model_exists": true,
  "model_size_bytes": 2971010875
}
```

**Action Required**: 
- Add detailed logging to sidecar model loading process
- Implement health check with model preload option
- Add timeout handling for model loading
- Investigate sidecar crash during inference requests

## Issue 4: Screenshot Permission Failures ⚠️ OPEN

**Symptom**: `Screenshot failed: screencapture command failed`

**Root Cause**: 
- macOS requires screen recording permission for `screencapture`
- Terminal/app running sootie may lack permissions
- Vision backend cannot capture app screenshots

**Evidence**: Log shows `screencapture command failed`

**Action Required**: 
- Document permission requirements in setup guide
- Add permission check in Vision backend initialization
- Provide fallback or graceful degradation

## Issue 5: AT Tree Backend Permission Failures ⚠️ OPEN

**Symptom**: `Failed to get running apps via osascript, falling back to empty list`

**Root Cause**: 
- AT tree backend uses osascript to get running apps
- osascript requires Accessibility permissions
- Permission denial causes fallback to empty list
- `find_target` cannot scope to app/window

**Evidence**: 
```
WARN Failed to get running apps via osascript, falling back to empty list
WARN at_tree backend error: target not found: AT tree failed
```

**Action Required**: 
- Same as Issue 4 (permission documentation and checks)

## Recommended Next Steps

1. **Model Loading Investigation**: 
   - Add verbose logging to `server.py` model loading
   - Test model loading independently
   - Add `--preload` option to sidecar startup

2. **Permission Handling**: 
   - Document all macOS permissions required
   - Add permission check during `sootie setup`
   - Provide user-friendly permission grant instructions

3. **Fallback Priority**: 
   - Document that Vision backend should be fallback, not primary
   - AT tree should be primary (if permissions granted)
   - Vision should only activate when AT tree fails

4. **Testing**: 
   - Create integration tests for Vision backend with mock sidecar
   - Add E2E tests for permission scenarios
   - Add health check tests for model loading states