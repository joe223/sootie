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
import base64
import hashlib
import json
import os
import signal
import sys
import threading
import tempfile
import time
import io
import atexit
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
        self.load_lock = threading.Lock()
        self.load_condition = threading.Condition(self.load_lock)
        self.load_in_progress = False
        self.load_error = None
        self.idle_timer = None
        self.idle_timeout = 600
        self.auth_token = None
        self.model_path = None
        self.model_loaded = False
        self.last_request_time = time.time()
        self.temp_files = []

state = ServerState()

def reset_idle_timer():
    """Reset idle timeout timer on each request."""
    if state.idle_timer:
        state.idle_timer.cancel()
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

def validate_auth_token(handler):
    """Validate X-Sootie-Auth header.
    
    Returns:
        bool: True if auth valid
        
    Sends 401 if invalid.
    """
    if not state.auth_token:
        return True
    
    provided_token = handler.headers.get("X-Sootie-Auth", "")
    
    if not provided_token:
        handler.send_error_response(401, {"error": "Missing X-Sootie-Auth header"})
        return False
    
    if provided_token != state.auth_token:
        handler.send_error_response(401, {"error": "Invalid auth token"})
        return False
    
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
                from mlx_vlm.prompt_utils import apply_prompt_template
                from mlx_vlm.utils import load_config
                
                state.model, state.processor = load(state.model_path)
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
                "pid": os.getpid(),
                "auth_enabled": state.auth_token is not None
            }
            
            self.send_json_response(200, health_data)
        else:
            self.send_error_response(404, {"error": "Not found"})
    
    def do_POST(self):
        """Handle POST requests (ground, detect, parse)."""
        reset_idle_timer()
        
        # Auth check
        if not validate_auth_token(self):
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
        # Load model if needed
        if not load_vlm():
            self.send_error_response(503, {"error": f"Model load failed: {state.load_error}"})
            return
        
        try:
            # Read request
            content_length = int(self.headers.get("Content-Length", 0))
            if content_length == 0:
                self.send_error_response(400, {"error": "Empty request body"})
                return
            
            body = self.rfile.read(content_length)
            data = json.loads(body)
            
            # Extract params
            img_b64 = data.get("image", "")
            prompt = data.get("prompt", "")
            screen_w = data.get("screen_width", 1920)
            screen_h = data.get("screen_height", 1080)
            
            if not img_b64 or not prompt:
                self.send_error_response(400, {"error": "Missing image or prompt"})
                return
            
            # Decode and validate
            img_data = base64.b64decode(img_b64)
            validate_image_input(img_data, max_size_mb=50)
            
            img = Image.open(io.BytesIO(img_data))
            
            # Format validation
            if img.format not in ["JPEG", "PNG"]:
                self.send_error_response(400, {"error": f"Invalid image format {img.format}, only JPEG/PNG allowed"})
                return
            
            # Resize if needed
            max_edge = 1280
            if max(img.size) > max_edge:
                scale = max_edge / max(img.size)
                img = img.resize((int(img.width * scale), int(img.height * scale)))
            
            # Save to temp file
            with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as f:
                img.convert("RGB").save(f.name, quality=85)
                temp_path = f.name
                state.temp_files.append(temp_path)
            
            # Generate grounding prompt
            grounding_prompt = f"Based on the screenshot, find: {prompt}"
            
            # Inference
            if sys.platform == "darwin":
                # macOS: MLX VLM
                from transformers import AutoTokenizer
                tokenizer = AutoTokenizer.from_pretrained(state.model_path)
                chat = [{"role": "user", "content": [
                    {"type": "image", "image": temp_path},
                    {"type": "text", "text": grounding_prompt}
                ]}]
                formatted = tokenizer.apply_chat_template(chat, tokenize=False, add_generation_prompt=True)
                
                result_text = ""
                from mlx_vlm import generate
                for chunk in generate(state.model, state.processor, formatted, image=temp_path, max_tokens=128, temp=0.0):
                    result_text += chunk.text if hasattr(chunk, 'text') else str(chunk)
            else:
                # Linux: Transformers
                inputs = state.processor(images=[img], text=[grounding_prompt], return_tensors="pt")
                output_ids = state.model.generate(**inputs, max_new_tokens=128)
                result_text = state.processor.decode(output_ids[0], skip_special_tokens=True)
            
            # Parse coordinates
            import re
            match = re.search(r'[\(\[]\s*([\d.]+)\s*,\s*([\d.]+)\s*[\)\]]', result_text)
            
            if match:
                nx, ny = float(match.group(1)), float(match.group(2))
                if nx <= 1.0 and ny <= 1.0:
                    # Normalized coordinates
                    x = nx * screen_w
                    y = ny * screen_h
                    confidence = 0.8
                else:
                    # Absolute coordinates
                    x = nx
                    y = ny
                    confidence = 0.6
                
                result = {"x": round(x, 1), "y": round(y, 1), "confidence": confidence}
            else:
                result = {
                    "x": screen_w / 2,
                    "y": screen_h / 2,
                    "confidence": 0.0,
                    "error": "Coordinate parse failed",
                    "raw_response": result_text
                }
            
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
    parser.add_argument("--auth-token", type=str, default=None, help="Authentication token")
    parser.add_argument("--model-path", type=str, default=None, help="Model directory path")
    parser.add_argument("--idle-timeout", type=int, default=600, help="Idle timeout in seconds")
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
    
    # Auth token
    state.auth_token = args.auth_token
    if state.auth_token:
        print(f"Authentication enabled with token")
    
    # Idle timeout
    state.idle_timeout = args.idle_timeout
    
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