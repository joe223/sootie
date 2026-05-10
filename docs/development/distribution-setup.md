# Distribution Setup Guide

This guide explains how to set up and maintain the various distribution channels for Sootie.

## Overview

| Platform | Method | File/Location |
|----------|--------|---------------|
| macOS/Linux | Install Script | `install.sh` |
| Windows | PowerShell Script | `install.ps1` |
| macOS | Homebrew | Separate tap repo |
| Windows | Scoop | `scoop/sootie.json` |
| All | Cargo | crates.io |

---

## Install Scripts

### Testing Locally

**Unix (macOS/Linux):**
```bash
# Test the script
./install.sh

# Test with specific version
SOOTIE_VERSION=v0.1.0 ./install.sh

# Test with custom install dir
INSTALL_DIR=~/.local/bin ./install.sh
```

**Windows (PowerShell):**
```powershell
# Test the script
.\install.ps1

# Test with specific version
$env:SOOTIE_VERSION = "v0.1.0"; .\install.ps1

# Test with custom install dir
$env:INSTALL_DIR = "C:\Tools"; .\install.ps1
```

---

## Homebrew (macOS/Linux)

### Setup Steps

1. **Create a separate tap repository** (e.g., `joe223/homebrew-sootie`):

```bash
# Create new repo
mkdir homebrew-sootie
cd homebrew-sootie
git init
```

2. **Add the formula to the tap repo:**

```bash
cp /path/to/sootie/homebrew-formula.rb homebrew-sootie/Formula/sootie.rb
```

3. **Commit and push:**

```bash
git add Formula/sootie.rb
git commit -m "Add Sootie formula v0.1.0"
git remote add origin https://github.com/joe223/homebrew-sootie.git
git push -u origin main
```

4. **Users can then install via:**

```bash
brew tap joe223/sootie
brew install sootie
```

### Updating the Formula

After each release:

1. Update the `version` in `Formula/sootie.rb`
2. Update the `sha256` values for each platform:

```bash
# Download the release binaries and compute checksums
shasum -a 256 sootie-macos-arm64
shasum -a 256 sootie-macos-x64
shasum -a 256 sootie-linux-x64
```

3. Update the formula with new checksums and commit.

---

## Scoop (Windows)

### Setup Steps

1. **Create a Scoop bucket repository** (e.g., `joe223/scoop-sootie`):

```bash
mkdir scoop-sootie
cd scoop-sootie
git init
```

2. **Add the manifest:**

```bash
cp /path/to/sootie/scoop/sootie.json scoop-sootie/bucket/sootie.json
```

3. **Commit and push:**

```bash
git add bucket/sootie.json
git commit -m "Add Sootie manifest v0.1.0"
git remote add origin https://github.com/joe223/scoop-sootie.git
git push -u origin master  # Scoop uses 'master' branch convention
```

4. **Users can then install via:**

```powershell
scoop bucket add sootie https://github.com/joe223/scoop-sootie
scoop install sootie
```

### Updating the Manifest

After each release:

1. Update the `version` field in `bucket/sootie.json`
2. Update the `url` to point to the new release

---

## Cargo (crates.io)

### Publishing

From the workspace root:

```bash
# Ensure you're logged in
cargo login

# Publish in order (dependencies first)
cd crates/sootie-core
cargo publish

cd ../sootie-mcp
cargo publish

cd ../sootie-cli
cargo publish
```

### Installation

Users can install via:

```bash
cargo install sootie-cli
```

---

## Release Checklist

Before tagging a new release:

1. [ ] Update version in all `Cargo.toml` files
2. [ ] Update version in `scoop/sootie.json`
3. [ ] Update version in `homebrew-formula.rb`
4. [ ] Run full test suite: `cargo test --workspace`
5. [ ] Build release binaries locally to verify
6. [ ] Update CHANGELOG.md

After tagging a release (`git tag vX.Y.Z && git push origin vX.Y.Z`):

1. [ ] Wait for GitHub Actions to build and publish release
2. [ ] Download binaries and compute SHA256 checksums
3. [ ] Update Homebrew formula with new checksums
4. [ ] Update Scoop manifest with new version
5. [ ] Publish to crates.io (if ready for public crate)

---

## Adding New Distribution Methods

### Linux Package Managers

Consider adding:

- **apt (Debian/Ubuntu)**: Create `.deb` package
- **dnf/yum (Fedora/RHEL)**: Create `.rpm` package
- **AUR (Arch Linux)**: Create PKGBUILD
- **Snap**: Create `snapcraft.yaml`
- **Flatpak**: Create `flatpak.json`

### Windows Package Managers

Consider adding:

- **Chocolatey**: Create `.nuspec` and packaging scripts
- **WinGet**: Submit manifest to microsoft/winget-pkgs

### macOS

- **MacPorts**: Create Portfile

---

## Security Considerations

1. **Always verify checksums** - Users should verify SHA256 checksums when downloading manually
2. **Sign binaries** - Consider code signing for Windows and macOS
3. **HTTPS only** - All download URLs must use HTTPS
4. **Pinned dependencies** - Use `--locked` when building from source
