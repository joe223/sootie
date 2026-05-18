# Distribution

Sootie has one package-manager install path per supported desktop platform.
The release workflow must publish every package entrypoint before a release is
considered installable.

## User Install Targets

| Platform | User command | Published by |
| --- | --- | --- |
| macOS | `brew install joe223/sootie/sootie` | `.github/workflows/release.yml` updates `joe223/homebrew-sootie` |
| Linux | `sudo apt-get install sootie` after adding the Sootie apt source | `.github/workflows/release.yml` publishes the `apt` branch |
| Windows | To be decided | No public package-manager promise yet |

Linux users add the apt source once:

```bash
curl -fsSL https://raw.githubusercontent.com/joe223/sootie/apt/sootie.list \
  | sudo tee /etc/apt/sources.list.d/sootie.list >/dev/null
sudo apt-get update
sudo apt-get install sootie
```

After installation, all platforms use the same setup flow:

```bash
sootie setup
sootie doctor --check
```

The default CLI output is human-readable. Automation that needs the structured
payload must add `--raw`, for example `sootie doctor --check --raw`.

## Release Workflow

Publishing a tag such as `v0.1.0` runs `.github/workflows/release.yml`. The tag
version must match the workspace version in `Cargo.toml`.

The workflow publishes these release artifacts:

```text
sootie-0.1.0-macos-arm64.tar.gz
sootie-0.1.0-macos-x64.tar.gz
sootie_0.1.0_amd64.deb
sootie.rb
```

It also updates:

- `joe223/homebrew-sootie` with a checksum-pinned formula for macOS arm64 and
  x64.
- The `apt` branch in this repository with `dists/`, `pool/`, `Release`,
  `Packages`, `Packages.gz`, `sootie.list`, and `sootie.sources`.

Set the `HOMEBREW_TAP_TOKEN` repository secret before publishing. The workflow
fails the release if it cannot update the Homebrew tap, because a GitHub release
without a working `brew install` path is not considered publishable.

## Local Package Builds

Build a macOS tarball for the current host:

```bash
scripts/build-macos-tarball.sh
```

Cross-targeted macOS release builds set both the Rust target and package arch:

```bash
SOOTIE_TARGET=x86_64-apple-darwin \
SOOTIE_PACKAGE_ARCH=x64 \
scripts/build-macos-tarball.sh
```

Generate a Homebrew formula after both macOS tarballs exist:

```bash
scripts/render-homebrew-formula.sh \
  0.1.0 \
  https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-0.1.0-macos-arm64.tar.gz \
  dist/sootie-0.1.0-macos-arm64.tar.gz \
  https://github.com/joe223/sootie/releases/download/v0.1.0/sootie-0.1.0-macos-x64.tar.gz \
  dist/sootie-0.1.0-macos-x64.tar.gz
```

Build a Debian package on Linux:

```bash
scripts/build-deb.sh
```

The script emits:

```text
dist/sootie_0.1.0_amd64.deb
```

Build apt repository metadata:

```bash
SOOTIE_APT_BASE_URL=https://raw.githubusercontent.com/joe223/sootie/apt \
  scripts/build-apt-repo.sh dist/sootie_0.1.0_amd64.deb
```

## Verification

Run these checks before publishing package artifacts:

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
scripts/build-macos-tarball.sh
bash -n scripts/build-deb.sh scripts/build-apt-repo.sh scripts/render-homebrew-formula.sh scripts/check-distribution-entrypoints.sh
```

After the release workflow finishes, verify the public installation entrypoints:

```bash
scripts/check-distribution-entrypoints.sh
```
