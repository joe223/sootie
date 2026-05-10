#!/bin/bash
# Sootie Development Bootstrap Script
# Usage: ./bootstrap.sh
# Initializes development environment and tools

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_step() {
    echo -e "${CYAN}[STEP]${NC} $1"
}

check_command() {
    local cmd="$1"
    local package="$2"
    
    if ! command -v "$cmd" >/dev/null 2>&1; then
        log_error "$cmd is not installed"
        log_info "Install with: $package"
        return 1
    fi
    
    log_success "$cmd is available"
    return 0
}

check_rust_toolchain() {
    log_step "Checking Rust toolchain..."
    
    check_command "rustc" "rustup (https://rustup.rs)" || return 1
    check_command "cargo" "rustup (https://rustup.rs)" || return 1
    
    local rust_version
    rust_version=$(rustc --version)
    log_info "Rust version: $rust_version"
    
    local cargo_version
    cargo_version=$(cargo --version)
    log_info "Cargo version: $cargo_version"
    
    return 0
}

install_cargo_tools() {
    log_step "Installing Cargo development tools..."
    
    local tools=(
        "cargo-commitlint:Commit message linting"
    )
    
    for tool_desc in "${tools[@]}"; do
        local tool="${tool_desc%%:*}"
        local desc="${tool_desc##*:}"
        
        if ! cargo install --list | grep -q "^${tool}"; then
            log_info "Installing ${tool} (${desc})..."
            cargo install "$tool" || {
                log_error "Failed to install ${tool}"
                return 1
            }
        else
            log_info "${tool} is already installed"
        fi
    done
    
    return 0
}

setup_git_hooks() {
    log_step "Setting up Git hooks..."
    
    if [ ! -d "$REPO_ROOT/.git" ]; then
        log_error "Not a Git repository. Please clone the repo first."
        return 1
    fi
    
    if ! command -v cargo >/dev/null 2>&1 || ! cargo commitlint --version >/dev/null 2>&1; then
        log_warn "cargo-commitlint not available, skipping commit-msg hook"
        return 0
    fi
    
    if [ -f "$REPO_ROOT/.git/hooks/commit-msg" ]; then
        log_info "Commit-msg hook already exists"
        read -rp "Reinstall? (y/N): " choice
        case "$choice" in
            y|Y )
                cargo commitlint uninstall --cwd "$REPO_ROOT" || true
                cargo commitlint install --cwd "$REPO_ROOT"
                log_success "Commit-msg hook reinstalled"
                ;;
            * )
                log_info "Keeping existing hook"
                ;;
        esac
    else
        cargo commitlint install --cwd "$REPO_ROOT"
        log_success "Commit-msg hook installed"
    fi
    
    return 0
}

build_project() {
    log_step "Building project..."
    
    cd "$REPO_ROOT"
    
    log_info "Running cargo build..."
    cargo build --workspace || {
        log_error "Build failed"
        return 1
    }
    
    log_success "Build completed successfully"
    return 0
}

run_tests() {
    log_step "Running tests..."
    
    cd "$REPO_ROOT"
    
    log_info "Running cargo test..."
    cargo test --workspace || {
        log_error "Tests failed"
        return 1
    }
    
    log_success "All tests passed"
    return 0
}

verify_commitlint() {
    log_step "Verifying commitlint setup..."
    
    cd "$REPO_ROOT"
    
    if [ ! -f "$REPO_ROOT/.commitlint.yaml" ]; then
        log_warn ".commitlint.yaml not found"
        return 0
    fi
    
    log_info "Testing commitlint with sample messages..."
    
    cargo commitlint check --cwd "$REPO_ROOT" --message "feat: test message" >/dev/null 2>&1 && {
        log_success "Valid message accepted"
    } || {
        log_error "Valid message rejected (unexpected)"
        return 1
    }
    
    cargo commitlint check --cwd "$REPO_ROOT" --message "invalid message" >/dev/null 2>&1 || {
        log_success "Invalid message rejected (expected)"
    }
    
    return 0
}

print_summary() {
    echo ""
    log_success "Bootstrap completed!"
    echo ""
    echo "Development environment is ready."
    echo ""
    echo "Quick commands:"
    echo "  Build:    cargo build --workspace"
    echo "  Test:     cargo test --workspace"
    echo "  Lint:     cargo clippy --all-targets --all-features -- -D warnings"
    echo "  Format:   cargo fmt"
    echo "  Run:      cargo run --package sootie-cli"
    echo ""
    echo "Commit message format:"
    echo "  <type>[scope]: <subject>"
    echo ""
    echo "Examples:"
    echo "  feat: add new feature"
    echo "  fix(mcp): correct selector resolution"
    echo "  docs: update installation guide"
    echo ""
    echo "Documentation:"
    echo "  docs/development/commit-guidelines.md"
    echo ""
}

main() {
    log_info "Sootie Development Bootstrap"
    log_info "============================="
    echo ""
    
    cd "$REPO_ROOT"
    
    check_rust_toolchain || exit 1
    echo ""
    
    install_cargo_tools || exit 1
    echo ""
    
    setup_git_hooks || log_warn "Git hooks setup incomplete"
    echo ""
    
    build_project || exit 1
    echo ""
    
    run_tests || log_warn "Tests have failures (review above)"
    echo ""
    
    verify_commitlint || log_warn "Commitlint verification incomplete"
    echo ""
    
    print_summary
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi