#!/bin/bash
set -e

echo "Running tests with coverage..."

if ! command -v cargo-llvm-cov &> /dev/null; then
    echo "Installing cargo-llvm-cov..."
    cargo install cargo-llvm-cov
fi

cargo llvm-cov clean --workspace

cargo llvm-cov --workspace --html

echo "Coverage report generated at: target/llvm-cov/html/index.html"

cargo llvm-cov --workspace --fail-under-lines 80 || echo "Coverage below 80% threshold"