#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

from huggingface_hub import snapshot_download


def main():
    parser = argparse.ArgumentParser(description="Download a Sootie vision model snapshot")
    parser.add_argument("--model-id", default="showlab/ShowUI-2B")
    parser.add_argument("--dest", required=True)
    parser.add_argument("--revision", default="main")
    args = parser.parse_args()

    dest = Path(args.dest).expanduser().resolve()
    dest.mkdir(parents=True, exist_ok=True)
    snapshot_download(
        repo_id=args.model_id,
        revision=args.revision,
        local_dir=dest,
        allow_patterns=[
            "*.json",
            "*.safetensors",
            "*.bin",
            "*.txt",
            "*.model",
            "*.py",
            "merges.txt",
            "vocab*",
            "tokenizer*",
            "preprocessor_config.json",
            "generation_config.json",
        ],
    )
    print(json.dumps({"model_id": args.model_id, "path": str(dest), "revision": args.revision}))


if __name__ == "__main__":
    main()
