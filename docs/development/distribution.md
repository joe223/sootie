# Distribution

Sootie has one package-manager install path per supported desktop platform.
The canonical release path is manual and does not depend on GitHub Actions.
Every published release must make the Homebrew tap, static release assets, and
apt repository reachable before the release is considered installable.

## User Install Targets

| Platform | User command | Published by |
| --- | --- | --- |
| macOS | `brew install joe223/sootie/sootie` | `scripts/release.sh --push` updates `joe223/homebrew-sootie` |
| Linux | `sudo apt-get install sootie` after adding the Sootie apt source | `scripts/release.sh --push` updates the `apt` branch |
| Windows | To be decided | No public package-manager promise yet |

Linux users add the apt source once:

```bash
sudo install -d -m 0755 /usr/share/keyrings
curl -fsSL https://raw.githubusercontent.com/joe223/sootie/apt/sootie-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/sootie-archive-keyring.gpg >/dev/null
sudo chmod 0644 /usr/share/keyrings/sootie-archive-keyring.gpg
curl -fsSL https://raw.githubusercontent.com/joe223/sootie/apt/sootie.sources \
  | sudo tee /etc/apt/sources.list.d/sootie.sources >/dev/null
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

## Manual Release Workflow

Run the release script from a clean Sootie checkout:

```bash
SOOTIE_APT_SIGNING_KEY=<gpg-key-id-or-fingerprint> \
SOOTIE_APT_PUBLIC_KEY_FILE=/path/to/sootie-archive-keyring.gpg \
  scripts/release.sh <version> --push
```

The version must match the workspace version in `Cargo.toml`. The `--push`
release path requires apt signing configuration so Linux users install with a
normal `signed-by` keyring instead of `trusted=yes`. The script builds and
publishes:

```text
release-assets/v<version>/sootie-<version>-macos-arm64.tar.gz
release-assets/v<version>/sootie-<version>-macos-x64.tar.gz
release-assets/v<version>/sootie_<version>_amd64.deb
apt/sootie-archive-keyring.gpg
apt/dists/stable/Release
apt/dists/stable/InRelease
apt/dists/stable/Release.gpg
apt/dists/stable/main/binary-amd64/Packages.gz
apt/sootie.list
apt/sootie.sources
```

It also updates `joe223/homebrew-sootie` with a checksum-pinned formula for
macOS arm64 and x64, then runs `scripts/check-distribution-entrypoints.sh`
against the public URLs. The public check downloads the package for the current
host platform when possible and runs `sootie tools --raw` from the published
binary so stale release assets fail the release gate.

The `release-assets` branch is versioned by directory. Do not create a new
asset branch for every version. Release assets are immutable: never overwrite
files under an existing `vX.Y.Z/` directory. Publish a new patch version instead.

The `apt` branch is the live apt repository. It can be replaced on every release
because apt clients use the current `stable` metadata.

The GitHub Actions workflow is optional. If account billing or runner capacity
prevents Actions from running, continue using the manual release script.

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
  <version> \
  https://raw.githubusercontent.com/joe223/sootie/release-assets/v<version>/sootie-<version>-macos-arm64.tar.gz \
  dist/sootie-<version>-macos-arm64.tar.gz \
  https://raw.githubusercontent.com/joe223/sootie/release-assets/v<version>/sootie-<version>-macos-x64.tar.gz \
  dist/sootie-<version>-macos-x64.tar.gz
```

Build a Debian package on Linux:

```bash
scripts/build-deb.sh
```

The script emits:

```text
dist/sootie_<version>_amd64.deb
```

Build apt repository metadata:

```bash
SOOTIE_APT_BASE_URL=https://raw.githubusercontent.com/joe223/sootie/apt \
SOOTIE_APT_SIGNING_KEY=<gpg-key-id-or-fingerprint> \
SOOTIE_APT_PUBLIC_KEY_FILE=/path/to/sootie-archive-keyring.gpg \
  scripts/build-apt-repo.sh dist/sootie_<version>_amd64.deb
```

For local dry-run metadata only, set `SOOTIE_APT_ALLOW_UNSIGNED=1`. Do not
publish unsigned apt metadata.

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

After publishing, verify the public installation entrypoints:

```bash
scripts/check-distribution-entrypoints.sh
```

The public check verifies the Homebrew formula version, release asset URLs, apt
source signing, apt metadata version, Debian package metadata, and the published
binary tool contract on compatible hosts. Local unsigned dry-run repositories can
be checked with `SOOTIE_REQUIRE_SIGNED_APT=0`; public release checks should keep
the default signed-source requirement.
