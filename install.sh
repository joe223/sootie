#!/bin/bash
# Sootie Cross-Platform Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/joe223/sootie/main/install.sh | bash

set -euo pipefail

# Cleanup temp directory on exit
TEMP_DIR=""
cleanup() {
    if [[ -n "${TEMP_DIR:-}" ]] && [[ -d "$TEMP_DIR" ]]; then
        rm -rf "$TEMP_DIR"
    fi
}
trap cleanup EXIT

REPO="joe223/sootie"
BINARY_NAME="sootie"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

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

detect_platform() {
    local os arch

    case "$(uname -s)" in
        Darwin*) os="macos" ;;
        Linux*)  os="linux" ;;
        CYG*|MINGW*|MSYS*) os="windows" ;;
        *) log_error "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x64" ;;
        arm64|aarch64) arch="arm64" ;;
        *) log_error "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac

    echo "${os}-${arch}"
}

get_download_url() {
    local platform="$1"
    local version="${SOOTIE_VERSION:-latest}"

    if [ "$version" = "latest" ]; then
        echo "https://github.com/${REPO}/releases/latest/download/sootie-${platform}"
    else
        echo "https://github.com/${REPO}/releases/download/${version}/sootie-${platform}"
    fi
}

download_binary() {
    local platform="$1"
    local url
    local dest_file

    url=$(get_download_url "$platform")
    TEMP_DIR=$(mktemp -d)
    dest_file="${TEMP_DIR}/sootie"

    log_info "Downloading Sootie for ${platform}..."
    log_info "URL: ${url}"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest_file" || {
            log_error "Failed to download from ${url}"
            exit 1
        }
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$url" -O "$dest_file" || {
            log_error "Failed to download from ${url}"
            exit 1
        }
    else
        log_error "Neither curl nor wget found. Please install one of them."
        exit 1
    fi

    chmod +x "$dest_file"
    echo "$dest_file"
}

install_binary() {
    local binary_path="$1"
    local install_path="${INSTALL_DIR}/${BINARY_NAME}"

    log_info "Installing to ${install_path}..."

    # Check if we need sudo
    if [ -d "$INSTALL_DIR" ] && [ ! -w "$INSTALL_DIR" ]; then
        log_warn "Installation directory requires sudo privileges"
        sudo mkdir -p "$INSTALL_DIR"
        sudo mv "$binary_path" "$install_path"
    else
        mkdir -p "$INSTALL_DIR"
        mv "$binary_path" "$install_path"
    fi

    log_success "Sootie installed successfully!"
}

verify_installation() {
    local install_path="${INSTALL_DIR}/${BINARY_NAME}"

    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        log_success "Sootie is available in PATH"
        log_info "Version: $(${BINARY_NAME} --version 2>/dev/null || echo 'unknown')"
    elif [ -x "$install_path" ]; then
        log_warn "Sootie is installed but not in PATH"
        log_info "Add ${INSTALL_DIR} to your PATH or run: ${install_path}"
    else
        log_error "Installation verification failed"
        exit 1
    fi
}

post_install_instructions() {
    echo ""
    log_success "Installation complete!"
    echo ""
    echo "Next steps:"
    echo "  1. Run: sootie setup"
    echo "  2. Configure your MCP client (Claude Code, Cursor, etc.)"
    echo ""
    echo "Documentation: https://github.com/${REPO}#readme"
}

main() {
    local platform
    local binary_path

    log_info "Sootie Installer"
    log_info "================"
    echo ""

    # Detect platform
    platform=$(detect_platform)
    log_info "Detected platform: ${platform}"

    # Handle Windows differently
    if [[ "$platform" == windows* ]]; then
        log_error "Windows installation via shell script not supported."
        log_info "Please use PowerShell installer instead:"
        log_info "  iwr -useb https://raw.githubusercontent.com/${REPO}/main/install.ps1 | iex"
        exit 1
    fi

    # Download and install
    binary_path=$(download_binary "$platform")
    install_binary "$binary_path"

    # Verify
    verify_installation

    # Instructions
    post_install_instructions
}

# Allow sourcing for testing, or run main
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
