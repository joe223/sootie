#!/usr/bin/env python3
import argparse
import base64
import io
import json
import os
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

MIN_PIXELS = 256 * 28 * 28
MAX_PIXELS = 1344 * 28 * 28

SYSTEM_PROMPT = (
    "Based on the screenshot of the page, I give a text description and you "
    "give its corresponding location. The coordinate represents a clickable "
    "location [x, y] for an element, which is a relative coordinate on the "
    "screenshot, scaled from 0 to 1."
)


class State:
    def __init__(self):
        self.model_path = None
        self.model = None
        self.processor = None
        self.loaded = False
        self.load_error = None


state = State()


def model_files_exist(path):
    if path is None:
        return False, "model path is not set"
    model_dir = Path(path)
    if not model_dir.exists():
        return False, f"model directory does not exist: {model_dir}"
    weight_patterns = ["*.safetensors", "pytorch_model*.bin", "*.gguf"]
    has_weights = any(model_dir.glob(pattern) for pattern in weight_patterns)
    has_config = (model_dir / "config.json").exists()
    if not has_weights:
        return False, f"model weights not found in {model_dir}"
    if not has_config:
        return False, f"config.json not found in {model_dir}"
    return True, None


def load_model():
    if state.loaded:
        return True
    ok, error = model_files_exist(state.model_path)
    if not ok:
        state.load_error = error
        return False
    try:
        import torch
        from transformers import AutoProcessor, Qwen2VLForConditionalGeneration

        cuda_available = torch.cuda.is_available()
        dtype = torch.float16 if cuda_available else torch.float32
        state.processor = AutoProcessor.from_pretrained(
            state.model_path,
            trust_remote_code=True,
            use_fast=False,
            size=None,
            min_pixels=MIN_PIXELS,
            max_pixels=MAX_PIXELS,
        )
        model_kwargs = {"dtype": dtype, "trust_remote_code": True}
        if cuda_available:
            model_kwargs["device_map"] = "auto"
        state.model = Qwen2VLForConditionalGeneration.from_pretrained(state.model_path, **model_kwargs)
        state.loaded = True
        state.load_error = None
        return True
    except Exception as error:
        state.load_error = f"{type(error).__name__}: {error}"
        return False


def parse_json_object(text):
    text = text.strip()
    if text.startswith("```"):
        text = re.sub(r"^```(?:json)?", "", text).strip()
        text = re.sub(r"```$", "", text).strip()
    start = text.find("{")
    end = text.rfind("}")
    if start < 0 or end < start:
        raise ValueError(f"model did not return JSON: {text[:200]}")
    return json.loads(text[start : end + 1])


def parse_coordinate_text(text):
    try:
        payload = parse_json_object(text)
        if "matches" in payload:
            return normalize_response(payload)
        if "point" in payload or ("x" in payload and "y" in payload):
            return normalize_response({"matches": [payload]})
    except Exception:
        pass

    numbers = re.findall(r"-?\d+(?:\.\d+)?", text)
    if len(numbers) < 2:
        raise ValueError(f"model did not return coordinates: {text[:200]}")
    x = float(numbers[0])
    y = float(numbers[1])
    if abs(x) > 1.0 or abs(y) > 1.0:
        x /= 1000.0
        y /= 1000.0
    return normalize_response(
        {
            "matches": [
                {
                    "label": "target",
                    "point": [x, y],
                    "bbox": {"x": x - 0.02, "y": y - 0.02, "width": 0.04, "height": 0.04},
                    "confidence": 0.8,
                }
            ]
        }
    )


def clamp(value, lo=0.0, hi=1.0):
    return max(lo, min(hi, float(value)))


def normalize_match(match):
    point = match.get("point") or match.get("position")
    bbox = match.get("bbox") or match.get("bounds") or match.get("box")
    if isinstance(point, dict):
        px = point.get("x")
        py = point.get("y")
    elif isinstance(point, list) and len(point) >= 2:
        px, py = point[0], point[1]
    else:
        px = match.get("x")
        py = match.get("y")
    if px is None or py is None:
        raise ValueError("match point is missing")
    if isinstance(bbox, list) and len(bbox) >= 4:
        bbox = {"x": bbox[0], "y": bbox[1], "width": bbox[2], "height": bbox[3]}
    if not isinstance(bbox, dict):
        bbox = {"x": clamp(px) - 0.02, "y": clamp(py) - 0.02, "width": 0.04, "height": 0.04}
    return {
        "label": str(match.get("label") or match.get("text") or match.get("name") or "target"),
        "confidence": clamp(match.get("confidence", match.get("score", 0.5))),
        "point": [clamp(px), clamp(py)],
        "bbox": {
            "x": clamp(bbox.get("x", bbox.get("left", 0.0))),
            "y": clamp(bbox.get("y", bbox.get("top", 0.0))),
            "width": clamp(bbox.get("width", 0.04)),
            "height": clamp(bbox.get("height", 0.04)),
        },
    }


def normalize_response(payload):
    matches = payload.get("matches", [])
    if not isinstance(matches, list):
        matches = []
    normalized = []
    for item in matches[:5]:
        try:
            normalized.append(normalize_match(item))
        except Exception:
            continue
    return {"matches": normalized}


def load_image(image_data):
    from PIL import Image

    Image.MAX_IMAGE_PIXELS = 100_000_000
    return Image.open(io.BytesIO(image_data)).convert("RGB")


def run_grounding(image, description):
    if not load_model():
        raise RuntimeError(state.load_error or "model failed to load")
    processor = state.processor
    model = state.model
    messages = [
        {
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_PROMPT},
                {"type": "image"},
                {"type": "text", "text": description},
            ],
        }
    ]
    text = processor.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
    inputs = processor(text=[text], images=[image], return_tensors="pt")
    if hasattr(model, "device"):
        inputs = {key: value.to(model.device) for key, value in inputs.items()}
    output = model.generate(
        **inputs,
        max_new_tokens=int(os.environ.get("SOOTIE_GROUNDING_MAX_TOKENS", "128")),
    )
    prompt_len = inputs["input_ids"].shape[-1]
    generated = output[:, prompt_len:]
    decoded = processor.batch_decode(generated, skip_special_tokens=True)[0]
    return parse_coordinate_text(decoded)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write("[%s] %s\n" % (self.log_date_time_string(), fmt % args))

    def send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path not in ("/health", "/"):
            self.send_json(404, {"error": "not found"})
            return
        ok, error = model_files_exist(state.model_path)
        self.send_json(
            200,
            {
                "ok": True,
                "model_ready": ok,
                "model_loaded": state.loaded,
                "model_path": str(state.model_path) if state.model_path else None,
                "model_error": error or state.load_error,
            },
        )

    def do_POST(self):
        if self.path != "/ground":
            self.send_json(404, {"error": "not found"})
            return
        start = time.perf_counter()
        try:
            length = int(self.headers.get("Content-Length", "0"))
            payload = json.loads(self.rfile.read(length).decode("utf-8"))
            image_data = base64.b64decode(payload["image"])
            image = load_image(image_data)
            description = str(payload.get("description") or "").strip()
            if not description:
                self.send_json(400, {"error": "description is required"})
                return
            result = run_grounding(image, description)
            result["timings"] = {"total_ms": round((time.perf_counter() - start) * 1000)}
            self.send_json(200, result)
        except Exception as error:
            self.send_json(503, {"error": str(error), "model_path": str(state.model_path)})


def main():
    parser = argparse.ArgumentParser(description="Sootie vision grounding sidecar")
    parser.add_argument("--port", type=int, default=9876)
    parser.add_argument("--model-path", default=os.environ.get("SOOTIE_VISION_MODEL_PATH"))
    parser.add_argument("--health-check", action="store_true")
    parser.add_argument("--preload", action="store_true")
    args = parser.parse_args()

    state.model_path = Path(args.model_path).expanduser() if args.model_path else None
    if args.preload and not load_model():
        print(json.dumps({"model_ready": False, "error": state.load_error}), file=sys.stderr)
        raise SystemExit(1)
    if args.health_check:
        ok, error = model_files_exist(state.model_path)
        print(
            json.dumps(
                {
                    "model_ready": ok,
                    "model_loaded": state.loaded,
                    "model_path": str(state.model_path),
                    "error": error or state.load_error,
                }
            )
        )
        raise SystemExit(0 if ok and (state.loaded or not args.preload) else 1)
    server = HTTPServer(("127.0.0.1", args.port), Handler)
    print(f"Sootie vision sidecar listening on http://127.0.0.1:{args.port}")
    print(f"Model path: {state.model_path}")
    server.serve_forever()


if __name__ == "__main__":
    main()
