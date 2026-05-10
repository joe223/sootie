# Release Strategy (No cargo install)

## Distribution Channels

**Primary:**
1. GitHub Releases (Pre-compiled binaries)
2. Homebrew (macOS)

**Future:**
3. AUR (Arch Linux)
4. Snap (Ubuntu)
5. Scoop (Windows)

---

## Phase 1: GitHub Releases (Primary)

### Setup

Already configured in `.github/workflows/release.yml`:

```yaml
# Triggered by: git tag v0.1.0 && git push origin v0.1.0
# Builds: macOS (x64, ARM), Linux (x64), Windows (x64)
# Uploads: Automatic to GitHub Releases
```

### First Release

```bash
# 1. Commit all changes
git add .
git commit -m "chore: add CI/CD workflows, update metadata"
git push

# 2. Create release tag
git tag v0.1.0 -m "Initial release"
git push origin v0.1.0

# 3. Wait for GitHub Actions (~10 minutes)
# Check: https://github.com/joe223/sootie/actions

# 4. Verify binaries uploaded
# https://github.com/joe223/sootie/releases/tag/v0.1.0
```

### User Installation (After Release)

```bash
# macOS Intel (all Intel Macs)
curl -L https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-macos-x64 -o sootie
chmod +x sootie

# macOS Apple Silicon (M1, M2, M3, M4, future M-series)
curl -L https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-macos-arm64 -o sootie
chmod +x sootie

# Linux (x86_64)
curl -L https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-linux-x64 -o sootie
chmod +x sootie

# Windows (x86_64)
# Download: sootie-windows-x64.exe
```

**Pros:**
- ✅ Fast download (~2 seconds)
- ✅ No Rust toolchain needed
- ✅ All platforms supported
- ✅ Automatic versioning

---

## Phase 2: Homebrew (macOS Priority)

### Create Custom Tap

```bash
# 1. Create GitHub repo: https://github.com/joe223/homebrew-sootie
# 2. Upload homebrew-formula.rb as: Formula/sootie.rb

# Update SHA256 after first GitHub release:
curl -L https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-macos-x64 -o sootie-macos-x64
shasum -a 256 sootie-macos-x64
# Update formula with computed SHA256
```

### Homebrew Formula Structure

Already created: `homebrew-formula.rb`

Key features:
- Downloads pre-compiled binary (no compilation)
- Supports Intel and ARM
- Optional build-from-source with `--with-build-from-source`
- Includes setup instructions in caveats

### User Installation

```bash
brew tap joe223/sootie https://github.com/joe223/homebrew-sootie
brew install sootie
sootie setup
```

**Pros:**
- ✅ Familiar installation for macOS users
- ✅ Auto-updates with `brew upgrade`
- ✅ Handles PATH and permissions
- ✅ Fast (~2 seconds download)

---

## Phase 3: Linux Package Repositories

### A. AUR (Arch Linux)

Create `PKGBUILD`:

```bash
# Maintainer: Joe Developer <joe@example.com>
pkgname=sootie-bin
pkgver=0.1.0
pkgrel=1
pkgdesc="Cross-platform computer-use for AI agents via MCP"
arch=('x86_64')
url="https://github.com/joe223/sootie"
license=('Apache-2.0')

source=("https://github.com/joe223/sootie/releases/download/v$pkgver/sootie-linux-x64")
sha256sums=('UPDATE_AFTER_RELEASE')

package() {
  install -Dm755 sootie-linux-x64 "$pkgdir/usr/bin/sootie"
}
```

Submit to AUR:
1. Create account on https://aur.archlinux.org
2. Submit package
3. Users install: `yay -S sootie-bin`

### B. Snap (Ubuntu/Debian)

Create `snap/snapcraft.yaml`:

```yaml
name: sootie
version: '0.1.0'
summary: Cross-platform computer-use for AI agents
description: |
  Sootie enables AI agents to see and operate desktop applications
  through MCP (Model Context Protocol).

grade: stable
confinement: classic

parts:
  sootie:
    plugin: dump
    source: https://github.com/joe223/sootie/releases/download/v$SNAPCRAFT_PROJECT_VERSION/sootie-linux-x64
    organize:
      sootie-linux-x64: usr/bin/sootie

apps:
  sootie:
    command: usr/bin/sootie
```

Publish to Snap Store:
1. Create account on https://snapcraft.io
2. `snapcraft login`
3. `snapcraft push sootie.snap`
4. Users install: `snap install sootie`

---

## Phase 4: Scoop (Windows)

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
      "sha256": "UPDATE_AFTER_RELEASE"
    }
  },
  "bin": "sootie.exe",
  "notes": "Run 'sootie setup' after installation."
}
```

Submit to Scoop bucket or create custom bucket.

---

## Release Workflow

### Simplest Approach (Recommended)

**Step 1: GitHub Release**
```bash
git tag v0.1.0
git push origin v0.1.0
# → GitHub Actions builds all binaries
# → Users download from releases
```

**Step 2: Homebrew Tap**
```bash
# Create: https://github.com/joe223/homebrew-sootie
# Upload formula (update SHA256)
# → macOS users: brew tap joe223/sootie && brew install sootie
```

**Step 3: Linux Repositories (Optional)**
```bash
# Submit AUR package
# Submit Snap package
# → Linux users have native options
```

---

## Advantages of Binary Distribution

| Aspect | Binary Distribution | cargo install |
|--------|---------------------|---------------|
| Speed | ✅ 2 seconds | ❌ 2-5 minutes (build) |
| Dependencies | ✅ None | ❌ Rust toolchain |
| User Experience | ✅ Download & run | ❌ Complex |
| Auto-update | ✅ brew upgrade, snap refresh | ❌ Manual reinstall |
| Version Management | ✅ Native | ✅ cargo |

---

## Next Steps

### Immediate (Day 1)

```bash
# 1. Push CI/CD workflows
git add .
git commit -m "chore: add CI/CD workflows, update metadata"
git push

# 2. Create first release
git tag v0.1.0 -m "Initial release"
git push origin v0.1.0

# 3. Monitor GitHub Actions
# https://github.com/joe223/sootie/actions

# Users can download binaries immediately!
```

### This Week (Week 1)

```bash
# 4. Create Homebrew tap
# https://github.com/joe223/homebrew-sootie

# 5. Calculate SHA256
curl -L https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-macos-x64 -o sootie-macos-x64
shasum -a 256 sootie-macos-x64

# 6. Update formula, upload to tap
# macOS users: brew install sootie
```

### Optional (Month 1)

```bash
# 7. Submit to AUR
# 8. Submit to Snap
# 9. Submit to Scoop
```

---

## Comparison: Distribution Priority

| Platform | Primary Method | Alternative |
|----------|---------------|-------------|
| macOS | Homebrew ✨ | GitHub download |
| Linux | GitHub download | AUR, Snap |
| Windows | GitHub download | Scoop |

**Focus on Homebrew for macOS** (best UX)
**Focus on GitHub Releases for all platforms** (universal)

---

## Summary

**No cargo install needed.** Users get:

- **macOS:** `brew tap joe223/sootie && brew install sootie` (2 seconds)
- **Linux:** Download binary from GitHub (2 seconds)
- **Windows:** Download .exe from GitHub (2 seconds)

**Fast, simple, no compilation required.**