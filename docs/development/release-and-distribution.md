# Release and Distribution Guide

## Current Status

✅ **Ready for Publishing:**
- Code is complete and tested
- Cargo workspace configured
- README has installation instructions (but not yet active)

⚠️ **Needs Setup:**
- crates.io publishing
- GitHub Actions CI/CD
- Homebrew formula
- Pre-compiled binaries
- Linux package repositories

---

## Phase 1: crates.io Publishing (cargo install)

### Prerequisites

1. **Add missing metadata to Cargo.toml**

```toml
[package]
name = "sootie-cli"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
authors = ["Joe Developer <joe@example.com>"]
description = "Cross-platform computer-use for AI agents via MCP"
repository = "https://github.com/joe223/sootie"
homepage = "https://github.com/joe223/sootie"
readme = "README.md"
keywords = ["mcp", "ai", "automation", "computer-use", "accessibility"]
categories = ["command-line-utilities", "development-tools"]

[package.metadata.docs.rs]
all-features = true
```

2. **Publish to crates.io**

```bash
# Login to crates.io (first time)
cargo login

# Dry run to check everything
cargo publish --dry-run -p sootie-cli

# Actually publish (order matters for dependencies)
cargo publish -p sootie-core
cargo publish -p sootie-mcp
cargo publish -p sootie-cli

# Users can then install with:
cargo install sootie-cli
```

**Pros:**
- ✅ Immediate availability to Rust users
- ✅ Version management handled by cargo
- ✅ Easy: `cargo install sootie-cli`

**Cons:**
- ⚠️ Requires Rust toolchain
- ⚠️ Compilation time (~2-5 minutes)

---

## Phase 2: GitHub Releases (Pre-compiled Binaries)

### Setup GitHub Actions Workflow

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact: sootie-macos-x64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: sootie-macos-arm64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: sootie-linux-x64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: sootie-windows-x64.exe

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}

      - name: Package
        run: |
          mkdir -p release
          if [ "${{ matrix.os }}" = "windows-latest" ]; then
            cp target/${{ matrix.target }}/release/sootie.exe release/${{ matrix.artifact }}
          else
            cp target/${{ matrix.target }}/release/sootie release/${{ matrix.artifact }}
            chmod +x release/${{ matrix.artifact }}
          fi

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: release/${{ matrix.artifact }}

  release:
    needs: build
    runs-on: ubuntu-latest

    steps:
      - name: Download artifacts
        uses: actions/download-artifact@v4

      - name: Create release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            sootie-macos-x64/*
            sootie-macos-arm64/*
            sootie-linux-x64/*
            sootie-windows-x64.exe/*
          generate_release_notes: true
```

### Usage

```bash
# Create release
git tag v0.1.0
git push origin v0.1.0

# Users download from:
# https://github.com/joe223/sootie/releases/tag/v0.1.0
```

**Pros:**
- ✅ No Rust toolchain needed
- ✅ Fast download (~2 seconds)
- ✅ Cross-platform binaries

---

## Phase 3: Homebrew Formula (brew install)

### Create Homebrew Formula

Create `Formula/sootie.rb` in homebrew-core or your tap:

```ruby
class Sootie < Formula
  desc "Cross-platform computer-use for AI agents via MCP"
  homepage "https://github.com/joe223/sootie"
  version "0.1.0"
  license "Apache-2.0"

  on_macos do
    on_intel do
      url "https://github.com/joe223/sootie/releases/download/v#{version}/sootie-macos-x64"
      sha256 "..." # Calculate after first release
    end
    on_arm do
      url "https://github.com/joe223/sootie/releases/download/v#{version}/sootie-macos-arm64"
      sha256 "..."
    end
  end

  on_linux do
    url "https://github.com/joe223/sootie/releases/download/v#{version}/sootie-linux-x64"
    sha256 "..."
  end

  def install
    bin.install "sootie-macos-x64" => "sootie" if Hardware::CPU.intel?
    bin.install "sootie-macos-arm64" => "sootie" if Hardware::CPU.arm?
    bin.install "sootie-linux-x64" => "sootie" if OS.linux?
  end

  def caveats
    <<~EOS
      Run `sootie setup` after installation to configure permissions and vision model.
    EOS
  end

  test do
    system "#{bin}/sootie", "--version"
  end
end
```

### Submit to Homebrew

**Option A: Official homebrew-core**
1. Fork https://github.com/Homebrew/homebrew-core
2. Add `Formula/sootie.rb`
3. Create PR

**Option B: Custom tap (faster)**
```bash
# Create your own tap
brew tap joe223/sootie https://github.com/joe223/homebrew-sootie

# Users install with:
brew install joe223/sootie/sootie
```

**Pros:**
- ✅ Very easy for macOS users
- ✅ Auto-updates with `brew upgrade`
- ✅ Handles dependencies

---

## Phase 4: Linux Package Repositories

### A. AUR (Arch Linux User Repository)

Create `sootie/PKGBUILD`:

```bash
# Maintainer: Joe Developer <joe@example.com>
pkgname=sootie
pkgver=0.1.0
pkgrel=1
pkgdesc="Cross-platform computer-use for AI agents via MCP"
arch=('x86_64')
url="https://github.com/joe223/sootie"
license=('Apache-2.0')

source=("https://github.com/joe223/sootie/releases/download/v$pkgver/sootie-linux-x64")
sha256sums=('...')

package() {
  install -Dm755 sootie-linux-x64 "$pkgdir/usr/bin/sootie"
}
```

Submit to AUR: https://aur.archlinux.org

### B. Snap (Ubuntu/Debian)

Create `snap/snapcraft.yaml`:

```yaml
name: sootie
version: '0.1.0'
summary: Cross-platform computer-use for AI agents
description: |
  Sootie enables AI agents to see and operate desktop applications
  through the Model Context Protocol (MCP).

grade: stable
confinement: classic

apps:
  sootie:
    command: usr/bin/sootie

parts:
  sootie:
    plugin: dump
    source: https://github.com/joe223/sootie/releases/download/v$SNAPCRAFT_PROJECT_VERSION/sootie-linux-x64
    organize:
      sootie-linux-x64: usr/bin/sootie
```

Publish: https://snapcraft.io

### C. Debian Package (.deb)

```bash
# Create debian package structure
mkdir -p pkg/DEBIAN pkg/usr/bin
cp sootie-linux-x64 pkg/usr/bin/sootie

cat > pkg/DEBIAN/control << EOF
Package: sootie
Version: 0.1.0
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Joe Developer <joe@example.com>
Description: Cross-platform computer-use for AI agents
EOF

dpkg-deb --build pkg sootie_0.1.0_amd64.deb
```

---

## Phase 5: Scoop (Windows)

Create `bucket/sootie.json`:

```json
{
  "version": "0.1.0",
  "description": "Cross-platform computer-use for AI agents via MCP",
  "homepage": "https://github.com/joe223/sootie",
  "license": "Apache-2.0",
  "architecture": {
    "64bit": {
      "url": "https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-windows-x64.exe",
      "sha256": "..."
    }
  },
  "bin": "sootie.exe",
  "notes": "Run 'sootie setup' after installation to configure permissions."
}
```

Add to Scoop bucket or submit to https://github.com/ScoopInstaller/Main

---

## Recommended Release Workflow

### Step 1: Immediate (Day 1)

```bash
# 1. Fix Cargo.toml metadata
# Add authors, description, keywords, etc.

# 2. Publish to crates.io
cargo publish -p sootie-core
cargo publish -p sootie-mcp
cargo publish -p sootie-cli

# Users can install immediately:
cargo install sootie-cli
```

### Step 2: Short-term (Week 1)

```bash
# 1. Setup GitHub Actions
mkdir -p .github/workflows
# Create release.yml (see above)

# 2. Create first release
git tag v0.1.0
git push origin v0.1.0

# Binaries auto-built and uploaded to GitHub Releases
```

### Step 3: Medium-term (Month 1)

```bash
# 1. Create Homebrew tap
# https://github.com/joe223/homebrew-sootie

# 2. Submit to AUR

# 3. Setup Snapcraft
```

---

## Version Management

### Semantic Versioning

```
v0.1.0 - Initial release
v0.1.1 - Bug fixes
v0.2.0 - New features (backward compatible)
v1.0.0 - Stable release
```

### Release Checklist

```markdown
- [ ] Update version in Cargo.toml
- [ ] Update CHANGELOG.md
- [ ] Run full test suite
- [ ] Build release binaries
- [ ] Test on all platforms
- [ ] Create git tag
- [ ] Push tag (trigger GitHub Actions)
- [ ] Update Homebrew formula
- [ ] Update AUR package
- [ ] Announce release
```

---

## Current Action Items

**Priority 1 (Now):**
1. Add metadata to Cargo.toml (authors, description, keywords)
2. Publish to crates.io

**Priority 2 (This Week):**
3. Setup GitHub Actions CI/CD
4. Create first GitHub release with binaries

**Priority 3 (This Month):**
5. Create Homebrew tap/formula
6. Submit to AUR
7. Setup Snap package

---

## Installation Methods Comparison

| Method | Platform | Speed | Ease | Auto-update |
|--------|----------|-------|------|-------------|
| cargo install | All | Slow (build) | Medium | Manual |
| GitHub Releases | All | Fast (download) | Medium | Manual |
| Homebrew | macOS | Fast | Easy ✨ | Yes |
| AUR | Arch | Fast | Medium | Yes |
| Snap | Ubuntu | Fast | Easy | Yes |
| Scoop | Windows | Fast | Easy | Yes |

---

## Next Steps

1. Add Cargo.toml metadata
2. Test `cargo publish --dry-run`
3. Publish to crates.io
4. Setup CI/CD
5. Create first release tag

After these steps, users can install via:
- `cargo install sootie-cli` (immediate)
- `brew install joe223/sootie/sootie` (after tap setup)
- Download from GitHub Releases (after CI/CD)