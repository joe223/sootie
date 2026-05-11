#!/usr/bin/env python3
"""
Vision Sidecar HTTP Server with authentication, lazy loading, and reliability features.

Features:
- Authentication: X-Sootie-Auth header validation
- Lazy model loading with threading.Lock + threading.Condition
- Idle timeout with auto-exit
- Signal handling (SIGTERM/SIGINT)
- Input validation (size limit, format whitelist)
- Health endpoint with detailed status
- Crash recovery support
"""

import argparse
import json
import os
import signal
import sys
import threading
import time
import atexit
import ast
import re
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path

try:
    from PIL import Image
    Image.MAX_IMAGE_PIXELS = 100_000_000
except ImportError:
    print("ERROR: Pillow not installed")
    sys.exit(1)

# Global state
class ServerState:
    def __init__(self):
        self.model = None
        self.processor = None
        self.tokenizer = None
        self.auth_token = None
        self.load_lock = threading.Lock()
        self.load_condition = threading.Condition(self.load_lock)
        self.load_in_progress = False
        self.load_error = None
        self.idle_timer = None
        self.idle_timeout = 600
        self.model_path = None
        self.model_loaded = False
        self.last_request_time = time.time()
        self.temp_files = []

state = ServerState()

GROUNDING_SYSTEM_PROMPT = (
    "Based on the screenshot of the window, I give a text description of a target. "
    "Return at most 5 likely visible target candidates, sorted from most likely to least likely. "
    "The target may be any UI object or visual element, such as a button, switch, field, icon, "
    "canvas item, color swatch, or another screen target. Coordinates and bounding boxes are "
    "relative to the screenshot and scaled from 0 to 1. Return compact JSON only in this shape: "
    "{\"matches\":[{\"label\":\"short description\",\"point\":[x,y],\"bbox\":{\"x\":x,\"y\":y,\"width\":w,\"height\":h},\"confidence\":0.9}]}. "
    "Do not repeat candidates. If no likely target is visible, return {\"matches\":[]}. Do not guess."
)

def now_ms():
    return time.perf_counter() * 1000.0

def elapsed_ms(start_ms):
    return round(now_ms() - start_ms, 2)

def grounding_max_tokens():
    raw = os.environ.get("SOOTIE_GROUNDING_MAX_TOKENS", "96")
    try:
        value = int(raw)
    except ValueError:
        return 160
    return max(32, min(value, 512))

def reset_idle_timer():
    """Reset idle timeout timer on each request."""
    if state.idle_timer:
        state.idle_timer.cancel()
        state.idle_timer = None
    if state.idle_timeout <= 0:
        return
    state.last_request_time = time.time()
    state.idle_timer = threading.Timer(state.idle_timeout, idle_exit)
    state.idle_timer.start()

def idle_exit():
    """Auto-exit after idle timeout."""
    print(f"Idle timeout ({state.idle_timeout}s) reached, exiting")
    cleanup_temp_files()
    os._exit(0)

def cleanup_temp_files():
    """Clean up tracked temporary files."""
    for path in state.temp_files:
        try:
            if os.path.exists(path):
                os.unlink(path)
        except Exception as e:
            print(f"Warning: failed to clean temp file {path}: {e}")
    state.temp_files.clear()

def signal_handler(signum, frame):
    """Handle SIGTERM/SIGINT for graceful shutdown."""
    print(f"Received signal {signum}, shutting down")
    cleanup_temp_files()
    if state.idle_timer:
        state.idle_timer.cancel()
    os._exit(0)

def validate_image_input(img_data, max_size_mb=50):
    """Validate image input before processing.
    
    Args:
        img_data: Decoded image bytes
        max_size_mb: Maximum size in MB
        
    Returns:
        bool: True if valid
        
    Raises:
        ValueError: If validation fails
    """
    size_mb = len(img_data) / (1024 * 1024)
    if size_mb > max_size_mb:
        raise ValueError(f"Image size {size_mb:.1f}MB exceeds limit {max_size_mb}MB")
    
    if len(img_data) == 0:
        raise ValueError("Empty image data")
    
    return True

def check_model_exists():
    """Check if model files exist and have minimum size.
    
    Returns:
        tuple: (exists: bool, size: int, error: str or None)
    """
    if not state.model_path:
        return (False, 0, "Model path not set")
    
    model_dir = Path(state.model_path)
    if not model_dir.exists():
        return (False, 0, f"Model directory {model_dir} does not exist")
    
    safetensors = model_dir / "model.safetensors"
    if not safetensors.exists():
        return (False, 0, "model.safetensors not found")
    
    size = safetensors.stat().st_size
    min_size = 2_500_000_000  # 2.5GB
    
    if size < min_size:
        return (False, size, f"model.safetensors too small ({size / 1e9:.2f}GB < 2.5GB)")
    
    return (True, size, None)

def wait_for_image_file(path, timeout_seconds=2.0):
    deadline = time.monotonic() + timeout_seconds
    last_error = None
    while time.monotonic() < deadline:
        try:
            if path.exists() and path.is_file():
                return True
        except OSError as error:
            last_error = error
        time.sleep(0.05)

    try:
        return path.exists() and path.is_file()
    except OSError as error:
        last_error = error
        print(f"[Vision] Image file wait failed for {path}: {last_error}")
        return False

def normalize_coordinate(value, size):
    if value <= 1.0:
        return value
    if size <= 0:
        raise ValueError("Image size must be positive")
    return value / size

def normalize_box_value(value, size):
    if size <= 0:
        raise ValueError("Image size must be positive")
    if value <= 1.0:
        return round(float(value), 6)
    return round(float(value) / size, 6)

def build_match(label, confidence, point, bbox):
    label_text = str(label).strip()
    if not label_text:
        raise ValueError("Grounding match label is required")
    if confidence is None:
        raise ValueError("Grounding match confidence is required")

    return {
        "label": label_text,
        "confidence": round(float(confidence), 6),
        "point": point,
        "bbox": bbox,
    }

def normalize_point_payload(payload, image_width, image_height):
    if isinstance(payload, (list, tuple)) and len(payload) >= 2:
        x = payload[0]
        y = payload[1]
        return {
            "x": round(normalize_coordinate(float(x), image_width), 6),
            "y": round(normalize_coordinate(float(y), image_height), 6),
        }

    if not isinstance(payload, dict) or "x" not in payload or "y" not in payload:
        raise ValueError(f"Invalid point payload: {payload}")
    x = payload["x"]
    y = payload["y"]

    return {
        "x": round(normalize_coordinate(float(x), image_width), 6),
        "y": round(normalize_coordinate(float(y), image_height), 6),
    }

def normalize_bbox_payload(payload, image_width, image_height):
    if isinstance(payload, (list, tuple)) and len(payload) >= 2:
        point = normalize_point_payload(payload, image_width, image_height)
        return bbox_around_point(point)
    if not isinstance(payload, dict) or not all(
        key in payload for key in ("x", "y", "width", "height")
    ):
        raise ValueError(f"Invalid bbox payload: {payload}")
    x = payload["x"]
    y = payload["y"]
    width = payload["width"]
    height = payload["height"]

    bbox = {
        "x": normalize_box_value(float(x), image_width),
        "y": normalize_box_value(float(y), image_height),
        "width": normalize_box_value(float(width), image_width),
        "height": normalize_box_value(float(height), image_height),
    }
    if bbox["width"] <= 0 or bbox["height"] <= 0:
        raise ValueError(f"Invalid bbox dimensions: {payload}")
    return bbox

def bbox_size_for_task(task_desc=""):
    desc = str(task_desc or "").lower()
    if any(keyword in desc for keyword in ("address", "url", "search", "text field", "textfield", "input")):
        return (0.42, 0.08)
    if any(keyword in desc for keyword in ("button", "tab", "toolbar")):
        return (0.12, 0.08)
    return (0.08, 0.08)

def bbox_around_point(point, width=None, height=None, task_desc=""):
    if width is None or height is None:
        width, height = bbox_size_for_task(task_desc)
    center_x = float(point["x"])
    center_y = float(point["y"])
    box_width = min(max(float(width), 0.001), 1.0)
    box_height = min(max(float(height), 0.001), 1.0)
    x = min(max(center_x - box_width / 2, 0.0), 1.0 - box_width)
    y = min(max(center_y - box_height / 2, 0.0), 1.0 - box_height)
    return {
        "x": round(x, 6),
        "y": round(y, 6),
        "width": round(box_width, 6),
        "height": round(box_height, 6),
    }

def normalize_match_payload(payload, image_width, image_height, task_desc=""):
    if isinstance(payload, (list, tuple)) and len(payload) >= 2:
        point = normalize_point_payload(payload, image_width, image_height)
        return build_match("vision match", 0.75, point, bbox_around_point(point, task_desc=task_desc))

    if not isinstance(payload, dict):
        raise ValueError(f"Invalid match payload: {payload}")

    point_payload = first_present(
        payload,
        ("point", "center", "coordinate", "coordinates", "location", "position"),
    )
    bbox_payload = first_present(payload, ("bbox", "box", "bounding_box", "boundingBox"))
    if point_payload is None and isinstance(bbox_payload, (list, tuple)):
        point_payload = bbox_payload
    if point_payload is None and isinstance(bbox_payload, dict):
        point_payload = point_from_bbox_payload(bbox_payload, image_width, image_height)
    point = normalize_point_payload(point_payload, image_width, image_height)

    if bbox_payload is None:
        bbox = bbox_around_point(point, task_desc=task_desc)
    else:
        try:
            bbox = normalize_bbox_payload(bbox_payload, image_width, image_height)
        except ValueError:
            bbox = bbox_around_point(point, task_desc=task_desc)

    return build_match(
        payload.get("label", "vision match"),
        payload.get("confidence", 0.75),
        point,
        bbox,
    )

def build_grounding_response(matches):
    return {"matches": matches}

def first_present(payload, keys):
    for key in keys:
        if key in payload:
            return payload.get(key)
    return None

def point_from_bbox_payload(payload, image_width, image_height):
    if not isinstance(payload, dict) or not all(
        key in payload for key in ("x", "y", "width", "height")
    ):
        return None
    bbox = normalize_bbox_payload(payload, image_width, image_height)
    return {
        "x": round(bbox["x"] + bbox["width"] / 2, 6),
        "y": round(bbox["y"] + bbox["height"] / 2, 6),
    }

def extract_generation_text(result):
    """Extract generated text from mlx_vlm result objects and stream chunks."""
    if result is None:
        return ""
    if isinstance(result, str):
        return result
    if hasattr(result, "text"):
        return str(result.text or "")
    return str(result)

def canonicalize_grounding_payload(payload):
    if isinstance(payload, dict):
        canonical = {}
        for key, value in payload.items():
            normalized_key = str(key).strip().strip("\"'").strip()
            canonical[normalized_key] = canonicalize_grounding_payload(value)
        return canonical
    if isinstance(payload, list):
        return [canonicalize_grounding_payload(item) for item in payload]
    if isinstance(payload, tuple):
        return tuple(canonicalize_grounding_payload(item) for item in payload)
    return payload

def parse_structured_grounding_payload(payload, image_width, image_height, task_desc=""):
    payload = canonicalize_grounding_payload(payload)
    if isinstance(payload, dict):
        if isinstance(payload.get("matches"), list):
            matches_payload = payload["matches"]
        elif any(
            key in payload
            for key in (
                "point",
                "position",
                "center",
                "coordinate",
                "coordinates",
                "location",
                "bbox",
                "box",
                "bounding_box",
            )
        ):
            try:
                matches_payload = [payload]
                matches = [
                    normalize_match_payload(match, image_width, image_height, task_desc)
                    for match in matches_payload
                ]
                return build_grounding_response(matches)
            except ValueError:
                fallback = infer_toolbar_field_match(payload, task_desc)
                if fallback is None:
                    raise
                return fallback
        else:
            fallback = infer_toolbar_field_match(payload, task_desc)
            if fallback is None:
                raise ValueError("Grounding response did not contain usable coordinates")
            return fallback
    elif isinstance(payload, list):
        matches_payload = payload
    else:
        raise ValueError("Grounding response did not contain usable coordinates")

    matches = [
        normalize_match_payload(match, image_width, image_height, task_desc)
        for match in matches_payload
    ]
    return build_grounding_response(matches)

def infer_toolbar_field_match(payload, task_desc):
    task = (task_desc or "").lower()
    looks_like_browser_field = any(
        keyword in task
        for keyword in ("url", "address", "search", "smart search", "textfield", "input field")
    )
    value = str(payload.get("value", "") or "")
    looks_like_url_value = any(
        marker in value.lower() for marker in ("http://", "https://", "www.", ".com", ".cn")
    )
    if not (looks_like_browser_field or looks_like_url_value):
        return None

    point = {"x": 0.5, "y": 0.08}
    bbox = {"x": 0.18, "y": 0.045, "width": 0.64, "height": 0.07}
    return build_grounding_response(
        [
            build_match(
                payload.get("label", "browser address/search field"),
                payload.get("confidence", 0.6),
                point,
                bbox,
            )
        ]
    )

def cleanup_grounding_text(text):
    cleaned = text.strip()
    cleaned = cleaned.replace("\u2018", "'").replace("\u2019", "'")
    cleaned = cleaned.replace("\u201c", '"').replace("\u201d", '"')
    # Some small VLM outputs contain keys like {' "matches": ...}. Normalize
    # those while preserving normal string values.
    cleaned = re.sub(r"'\s*\"([A-Za-z_][A-Za-z0-9_]*)\"\s*:", r"'\1':", cleaned)
    cleaned = re.sub(r"\"\s*'([A-Za-z_][A-Za-z0-9_]*)'\s*:", r"'\1':", cleaned)
    return re.sub(r"(['\"])\s*\"([A-Za-z_][A-Za-z0-9_]*)\"\s*\1\s*:", r"'\2':", cleaned)

def extract_structured_candidates(text):
    candidates = [text]
    if "```" in text:
        fenced = re.findall(r"```(?:json)?\s*(.*?)```", text, re.DOTALL | re.IGNORECASE)
        candidates.extend(segment.strip() for segment in fenced if segment.strip())

    stack = []
    start = None
    pairs = {"{": "}", "[": "]"}
    for index, char in enumerate(text):
        if char in pairs:
            if not stack:
                start = index
            stack.append(pairs[char])
        elif stack and char == stack[-1]:
            stack.pop()
            if not stack and start is not None:
                candidates.append(text[start : index + 1].strip())
                start = None
    return candidates

def parse_points_from_text(text, image_width, image_height, task_desc=""):
    matches = []
    point_patterns = [
        r"point['\"]?\s*:\s*\{[^}]*['\"]?x['\"]?\s*:\s*([0-9.]+)[^}]*['\"]?y['\"]?\s*:\s*([0-9.]+)",
        r"\[\s*([0-9.]+)\s*,\s*([0-9.]+)\s*\]",
    ]
    for pattern in point_patterns:
        for raw_x, raw_y in re.findall(pattern, text, re.IGNORECASE | re.DOTALL):
            try:
                point = normalize_point_payload([raw_x, raw_y], image_width, image_height)
                matches.append(build_match("vision match", 0.7, point, bbox_around_point(point, task_desc=task_desc)))
            except (TypeError, ValueError):
                continue
    if not matches:
        raise ValueError("Grounding response did not contain usable coordinates")
    return build_grounding_response(matches)

def parse_grounding_response(result_text, image_width, image_height, task_desc=""):
    """Parse model output into normalized point and bbox matches."""
    text = cleanup_grounding_text(result_text)

    for candidate in extract_structured_candidates(text):
        candidate = cleanup_grounding_text(candidate)
        for parser in (json.loads, ast.literal_eval):
            try:
                parsed = parser(candidate)
                return parse_structured_grounding_payload(
                    parsed,
                    image_width,
                    image_height,
                    task_desc,
                )
            except (json.JSONDecodeError, ValueError, SyntaxError, TypeError):
                continue

    try:
        return parse_points_from_text(text, image_width, image_height, task_desc)
    except ValueError:
        pass

    raise ValueError(f"Grounding response did not contain usable coordinates: {result_text[:200]}")

def load_vlm():
    """Load VLM model lazily with thread safety.
    
    Returns:
        bool: True if successful
        
    Sets load_error on failure.
    """
    if state.model_loaded:
        return True
    
    with state.load_lock:
        if state.model_loaded:
            return True
        
        if state.load_in_progress:
            # Wait for ongoing load (max 30s)
            state.load_condition.wait(timeout=30)
            return state.model_loaded and not state.load_error
        
        state.load_in_progress = True
        
        try:
            # Pre-check model
            exists, size, error = check_model_exists()
            if not exists:
                state.load_error = error or "Model not found"
                state.load_in_progress = False
                state.load_condition.notify_all()
                return False
            
            print(f"Loading VLM model from {state.model_path}...")
            
            if sys.platform == "darwin":
                # macOS: MLX VLM
                from mlx_vlm import load, generate
                from mlx_vlm.utils import load_config
                from transformers import AutoTokenizer

                state.model, state.processor = load(state.model_path)
                state.tokenizer = AutoTokenizer.from_pretrained(state.model_path)
                state.model_loaded = True
                print("MLX VLM model loaded successfully")
            else:
                # Linux: Transformers
                from transformers import AutoModelForCausalLM, AutoProcessor
                
                state.processor = AutoProcessor.from_pretrained(state.model_path, trust_remote_code=True)
                state.model = AutoModelForCausalLM.from_pretrained(
                    state.model_path, 
                    trust_remote_code=True,
                    torch_dtype="auto",
                    device_map="auto"
                )
                state.model_loaded = True
                print("Transformers model loaded successfully")
            
            state.load_in_progress = False
            state.load_condition.notify_all()
            return True
            
        except Exception as e:
            state.load_error = str(e)
            state.load_in_progress = False
            state.load_condition.notify_all()
            print(f"ERROR: Model load failed: {e}")
            return False

class VisionHandler(BaseHTTPRequestHandler):
    """HTTP request handler with auth and input validation."""
    
    def log_message(self, format, *args):
        """Log to stderr for debugging."""
        sys.stderr.write("%s - - [%s] %s\n" %
                         (self.address_string(),
                          self.log_date_time_string(),
                          format % args))
    
    def send_error_response(self, code, data):
        """Send JSON error response."""
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())
    
    def send_json_response(self, code, data):
        """Send JSON response."""
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())
    
    def do_GET(self):
        """Handle GET requests (health check)."""
        if self.path == "/health":
            reset_idle_timer()
            
            # Check model status
            exists, size, model_error = check_model_exists()
            
            status = "idle"
            if state.load_in_progress:
                status = "loading"
            elif state.model_loaded:
                status = "ready"
            elif state.load_error:
                status = "error"
            
            health_data = {
                "status": status,
                "status_detail": {
                    "idle": "Server running, model not loaded",
                    "loading": "Model loading in progress",
                    "ready": "Model loaded and ready for inference",
                    "error": state.load_error or "Unknown error"
                }.get(status, "Unknown"),
                "version": "1.0.0",
                "models_loaded": state.model_loaded,
                "model_path": str(state.model_path) if state.model_path else None,
                "model_exists": exists,
                "model_size_bytes": size,
                "vlm_load_error": state.load_error,
                "idle_timeout": state.idle_timeout,
                "pid": os.getpid()
            }
            
            self.send_json_response(200, health_data)
        elif self.path == "/warmup":
            reset_idle_timer()
            started = now_ms()
            if load_vlm():
                self.send_json_response(200, {
                    "status": "ready",
                    "models_loaded": state.model_loaded,
                    "duration_ms": elapsed_ms(started),
                })
            else:
                self.send_error_response(503, {
                    "status": "error",
                    "error": state.load_error or "Model preload failed",
                    "duration_ms": elapsed_ms(started),
                })
        else:
            self.send_error_response(404, {"error": "Not found"})
    
    def do_POST(self):
        """Handle POST requests (ground, detect, parse)."""
        reset_idle_timer()

        if state.auth_token is not None:
            auth_header = self.headers.get("X-Sootie-Auth")
            if auth_header != state.auth_token:
                self.send_error_response(401, {"error": "Missing or invalid X-Sootie-Auth header"})
                return
        
        # Route handling
        if self.path == "/ground":
            self.handle_ground()
        elif self.path == "/detect":
            self.send_error_response(501, {"error": "YOLO detection not implemented"})
        elif self.path == "/parse":
            self.send_error_response(501, {"error": "Screen parsing not implemented"})
        else:
            self.send_error_response(404, {"error": "Not found"})
    
    def handle_ground(self):
        """Handle /ground endpoint for coordinate grounding."""
        total_start = now_ms()
        timings = {}
        debug_log = "/tmp/vision_flow.txt"
        with open(debug_log, "a") as df:
            df.write(f"\n=== handle_ground called ===\n")
        
        # Load model if needed
        load_start = now_ms()
        if not load_vlm():
            with open(debug_log, "a") as df:
                df.write(f"Model load failed: {state.load_error}\n")
            self.send_error_response(503, {"error": f"Model load failed: {state.load_error}"})
            return
        timings["model_load_ms"] = elapsed_ms(load_start)
        
        with open(debug_log, "a") as df:
            df.write(f"Model loaded successfully\n")
        
        try:
            # Read request
            request_start = now_ms()
            content_length = int(self.headers.get("Content-Length", 0))
            if content_length == 0:
                self.send_error_response(400, {"error": "Empty request body"})
                return
            
            body = self.rfile.read(content_length)
            data = json.loads(body)
            timings["request_read_ms"] = elapsed_ms(request_start)
            
            # Extract params
            task_desc = data.get("task_desc", "")
            local_image_path = data.get("local_image_path", "")

            if not task_desc or not local_image_path:
                self.send_error_response(400, {"error": "Missing task_desc or local_image_path"})
                return

            image_path = Path(local_image_path)
            if not wait_for_image_file(image_path):
                self.send_error_response(400, {"error": f"Image file not found: {local_image_path}"})
                return

            image_start = now_ms()
            img = Image.open(image_path)
            if img.format not in ["JPEG", "PNG"]:
                self.send_error_response(400, {"error": f"Invalid image format {img.format}, only JPEG/PNG allowed"})
                return

            validate_image_input(image_path.read_bytes(), max_size_mb=10)
            timings["image_open_validate_ms"] = elapsed_ms(image_start)
            
            # Debug log file
            debug_log = "/tmp/vision_debug.txt"
            with open(debug_log, "a") as df:
                df.write(f"\n=== NEW REQUEST ===\n")
                df.write(f"Task: {task_desc}\n")
                df.write(f"Image path: {local_image_path}\n")
                df.write(f"Image size: {img.size}\n")
            
            if sys.platform == "darwin":
                # macOS: MLX VLM
                prompt_start = now_ms()
                tokenizer = state.tokenizer
                if tokenizer is None:
                    from transformers import AutoTokenizer
                    tokenizer = AutoTokenizer.from_pretrained(state.model_path)
                    state.tokenizer = tokenizer
                chat = [{"role": "user", "content": [
                    {"type": "text", "text": GROUNDING_SYSTEM_PROMPT},
                    {"type": "image", "image": local_image_path},
                    {"type": "text", "text": task_desc}
                ]}]
                formatted = tokenizer.apply_chat_template(chat, tokenize=False, add_generation_prompt=True)
                timings["prompt_ms"] = elapsed_ms(prompt_start)
                
                with open(debug_log, "a") as df:
                    df.write(f"Formatted prompt: {formatted[:200]}\n")
                
                from mlx_vlm import generate
                try:
                    with open(debug_log, "a") as df:
                        df.write("Using non-streaming API...\n")
                    
                    infer_start = now_ms()
                    result_text = extract_generation_text(
                        generate(
                            state.model,
                            state.processor,
                            formatted,
                            image=local_image_path,
                            max_tokens=grounding_max_tokens(),
                            temp=0.0,
                            verbose=False,
                        )
                    )
                    timings["infer_ms"] = elapsed_ms(infer_start)
                    
                    with open(debug_log, "a") as df:
                        df.write(f"Non-streaming result: '{result_text}'\n")
                        df.write(f"Result type: {type(result_text)}\n")
                except Exception as e:
                    with open(debug_log, "a") as df:
                        df.write(f"ERROR: {type(e).__name__}: {e}\n")
                        import traceback
                        df.write(traceback.format_exc())
                    self.send_error_response(500, {"error": f"Model inference failed: {e}"})
                    return
            else:
                # Linux: Transformers
                prompt_start = now_ms()
                prompt = f"{GROUNDING_SYSTEM_PROMPT}\n{task_desc}"
                inputs = state.processor(images=[img], text=[prompt], return_tensors="pt")
                timings["prompt_ms"] = elapsed_ms(prompt_start)
                infer_start = now_ms()
                output_ids = state.model.generate(**inputs, max_new_tokens=grounding_max_tokens())
                timings["infer_ms"] = elapsed_ms(infer_start)
                decode_start = now_ms()
                result_text = state.processor.decode(output_ids[0], skip_special_tokens=True)
                timings["decode_ms"] = elapsed_ms(decode_start)

            # Log raw response for debugging
            print(f"[Vision] Raw model response: {result_text[:500]}")

            parse_start = now_ms()
            result = parse_grounding_response(result_text, img.width, img.height, task_desc)
            timings["parse_ms"] = elapsed_ms(parse_start)
            timings["total_ms"] = elapsed_ms(total_start)
            result["timings"] = timings
            print(f"[Vision] Timings: {timings}")
            print(f"[Vision] Final result: {result}")
            
            self.send_json_response(200, result)
            
        except json.JSONDecodeError as e:
            self.send_error_response(400, {"error": f"Invalid JSON: {e}"})
        except ValueError as e:
            self.send_error_response(400, {"error": str(e)})
        except Exception as e:
            self.send_error_response(500, {"error": f"Internal error: {e}"})

def run_health_check():
    """Run health check and exit."""
    exists, size, error = check_model_exists()
    
    if not exists:
        print(f"ERROR: {error}")
        sys.exit(1)
    
    print(f"OK: Model exists at {state.model_path}, size {size / 1e9:.2f}GB")
    sys.exit(0)

def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Vision sidecar server")
    parser.add_argument("--port", type=int, default=9876, help="HTTP port")
    parser.add_argument("--model-path", type=str, default=None, help="Model directory path")
    parser.add_argument("--idle-timeout", type=int, default=600, help="Idle timeout in seconds; 0 disables auto-exit")
    parser.add_argument("--health-check", action="store_true", help="Run health check and exit")
    parser.add_argument("--preload", action="store_true", help="Preload model at startup")
    parser.add_argument("--version", action="store_true", help="Print version and exit")
    
    args = parser.parse_args()
    
    if args.version:
        print("vision-sidecar 1.0.0")
        sys.exit(0)
    
    # Setup model path
    if args.model_path:
        state.model_path = Path(args.model_path)
    else:
        env_path = os.environ.get("SOOTIE_VISION_MODEL_PATH")
        if env_path:
            state.model_path = Path(env_path)
        else:
            state.model_path = Path.home() / ".local" / "share" / "sootie" / "models" / "ShowUI-2B"
    
    # Idle timeout
    state.idle_timeout = args.idle_timeout
    state.auth_token = os.environ.get("SOOTIE_SIDECAR_AUTH_TOKEN")
    
    # Health check mode
    if args.health_check:
        run_health_check()
    
    # Setup signal handlers
    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)
    
    # Cleanup on exit
    atexit.register(cleanup_temp_files)
    
    # Preload if requested
    if args.preload:
        print("Preloading model...")
        if not load_vlm():
            print(f"ERROR: Model preload failed: {state.load_error}")
            sys.exit(1)
    
    # Start server
    server = HTTPServer(("127.0.0.1", args.port), VisionHandler)
    
    print(f"Vision sidecar running on port {args.port}")
    print(f"Model path: {state.model_path}")
    print(f"Idle timeout: {args.idle_timeout}s")
    print(f"Auth enabled: {state.auth_token is not None}")
    
    # Start idle timer
    reset_idle_timer()

    server.serve_forever()

if __name__ == "__main__":
    main()
