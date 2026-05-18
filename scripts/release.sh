#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${SOOTIE_REPO:-joe223/sootie}"
ASSETS_BRANCH="${SOOTIE_RELEASE_ASSETS_BRANCH:-release-assets}"
APT_BRANCH="${SOOTIE_APT_BRANCH:-apt}"
HOMEBREW_TAP_DIR="${SOOTIE_HOMEBREW_TAP_DIR:-$HOME/git/homebrew-sootie}"
PUSH=0
SKIP_BUILD=0
SKIP_PUBLIC_CHECK=0

usage() {
  cat <<EOF
Usage: scripts/release.sh <version> [options]

Options:
  --push                  Commit and push release-assets, apt, tap, and tag.
  --skip-build            Reuse existing dist artifacts.
  --skip-public-check     Do not check public URLs after pushing.
  --homebrew-tap-dir DIR  Local checkout of joe223/homebrew-sootie.
  -h, --help              Show this help.

Environment:
  SOOTIE_REPO                  GitHub repo slug, default: joe223/sootie
  SOOTIE_RELEASE_ASSETS_BRANCH Asset branch, default: release-assets
  SOOTIE_APT_BRANCH            Apt repository branch, default: apt
  SOOTIE_HOMEBREW_TAP_DIR      Homebrew tap checkout path
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ $# -lt 1 ]]; then
  usage >&2
  exit 2
fi

VERSION="$1"
shift
VERSION="${VERSION#v}"

if [[ -z "$VERSION" || "$VERSION" == -* ]]; then
  echo "Missing release version." >&2
  usage >&2
  exit 2
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --push)
      PUSH=1
      ;;
    --skip-build)
      SKIP_BUILD=1
      ;;
    --skip-public-check)
      SKIP_PUBLIC_CHECK=1
      ;;
    --homebrew-tap-dir)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --homebrew-tap-dir." >&2
        usage >&2
        exit 2
      fi
      HOMEBREW_TAP_DIR="$2"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

log() {
  printf '[release] %s\n' "$1"
}

die() {
  printf '[release] error: %s\n' "$1" >&2
  exit 1
}

sha256_file() {
  local file="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{ print $1 }'
  else
    shasum -a 256 "$file" | awk '{ print $1 }'
  fi
}

ensure_clean_repo() {
  local repo_dir="$1"
  local name="$2"

  if [[ -n "$(git -C "$repo_dir" status --porcelain)" ]]; then
    die "$name working tree is not clean: $repo_dir"
  fi
}

ensure_version_matches_workspace() {
  local workspace_version
  workspace_version="$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")"
  if [[ "$VERSION" != "$workspace_version" ]]; then
    die "requested version $VERSION does not match Cargo.toml version $workspace_version"
  fi
}

commit_if_changed() {
  local repo_dir="$1"
  local message="$2"

  git -C "$repo_dir" add --all
  if git -C "$repo_dir" diff --cached --quiet; then
    log "no commit needed in $repo_dir"
  else
    git -C "$repo_dir" commit -m "$message"
  fi
}

copy_immutable_asset() {
  local source="$1"
  local dest="$2"

  if [[ -f "$dest" ]]; then
    if [[ "$(sha256_file "$source")" != "$(sha256_file "$dest")" ]]; then
      die "release asset already exists with different contents: $dest"
    fi
    log "asset already exists and matches: $dest"
    return
  fi

  cp "$source" "$dest"
}

prepare_branch_worktree() {
  local branch="$1"
  local worktree="$2"

  if git -C "$ROOT" show-ref --verify --quiet "refs/heads/$branch"; then
    git -C "$ROOT" worktree add "$worktree" "$branch" >/dev/null
  elif git -C "$ROOT" ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
    git -C "$ROOT" fetch origin "$branch:$branch" >/dev/null
    git -C "$ROOT" worktree add "$worktree" "$branch" >/dev/null
  else
    git -C "$ROOT" worktree add --detach "$worktree" HEAD >/dev/null
    git -C "$worktree" switch --orphan "$branch" >/dev/null
    git -C "$worktree" rm -rf . >/dev/null 2>&1 || true
  fi
}

publish_assets_branch() {
  local worktree
  worktree="$(mktemp -d /tmp/sootie-release-assets.XXXXXX)"
  prepare_branch_worktree "$ASSETS_BRANCH" "$worktree"

  mkdir -p "$worktree/v$VERSION"
  copy_immutable_asset \
    "$ROOT/dist/sootie-$VERSION-macos-arm64.tar.gz" \
    "$worktree/v$VERSION/sootie-$VERSION-macos-arm64.tar.gz"
  copy_immutable_asset \
    "$ROOT/dist/sootie-$VERSION-macos-x64.tar.gz" \
    "$worktree/v$VERSION/sootie-$VERSION-macos-x64.tar.gz"
  copy_immutable_asset \
    "$ROOT/dist/sootie_${VERSION}_amd64.deb" \
    "$worktree/v$VERSION/sootie_${VERSION}_amd64.deb"
  commit_if_changed "$worktree" "chore: publish Sootie $VERSION assets"
  git -C "$worktree" push origin "$ASSETS_BRANCH"
  git -C "$ROOT" worktree remove "$worktree"
}

publish_apt_branch() {
  local worktree
  worktree="$(mktemp -d /tmp/sootie-apt.XXXXXX)"
  prepare_branch_worktree "$APT_BRANCH" "$worktree"

  find "$worktree" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
  cp -R "$ROOT/dist/apt/." "$worktree/"
  touch "$worktree/.nojekyll"
  commit_if_changed "$worktree" "chore: publish Sootie apt repository"
  git -C "$worktree" push origin "$APT_BRANCH"
  git -C "$ROOT" worktree remove "$worktree"
}

publish_homebrew_tap() {
  [[ -d "$HOMEBREW_TAP_DIR/.git" ]] || die "Homebrew tap checkout not found: $HOMEBREW_TAP_DIR"
  ensure_clean_repo "$HOMEBREW_TAP_DIR" "Homebrew tap"

  mkdir -p "$HOMEBREW_TAP_DIR/Formula"
  cp "$ROOT/dist/homebrew/sootie.rb" "$HOMEBREW_TAP_DIR/Formula/sootie.rb"
  commit_if_changed "$HOMEBREW_TAP_DIR" "fix: publish Sootie $VERSION formula"
  git -C "$HOMEBREW_TAP_DIR" push origin "$(git -C "$HOMEBREW_TAP_DIR" branch --show-current)"
}

publish_tag() {
  local tag="v$VERSION"

  if git -C "$ROOT" ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
    log "tag $tag already exists on origin"
    return
  fi
  if ! git -C "$ROOT" show-ref --verify --quiet "refs/tags/$tag"; then
    git -C "$ROOT" tag -a "$tag" -m "Sootie $VERSION"
  fi
  git -C "$ROOT" push origin "$tag"
}

ensure_version_matches_workspace
ensure_clean_repo "$ROOT" "Sootie"

if [[ "$SKIP_BUILD" == "0" ]]; then
  log "building macOS arm64 tarball"
  SOOTIE_TARGET=aarch64-apple-darwin \
  SOOTIE_PACKAGE_ARCH=arm64 \
    "$ROOT/scripts/build-macos-tarball.sh"

  log "building macOS x64 tarball"
  SOOTIE_TARGET=x86_64-apple-darwin \
  SOOTIE_PACKAGE_ARCH=x64 \
    "$ROOT/scripts/build-macos-tarball.sh"

  log "building Linux amd64 Debian package"
  SOOTIE_DEB_ARCH=amd64 "$ROOT/scripts/build-deb.sh"
fi

ASSET_BASE="https://raw.githubusercontent.com/$REPO/$ASSETS_BRANCH/v$VERSION"
APT_BASE="https://raw.githubusercontent.com/$REPO/$APT_BRANCH"

log "building apt repository metadata"
SOOTIE_APT_BASE_URL="$APT_BASE" \
  "$ROOT/scripts/build-apt-repo.sh" "$ROOT/dist/sootie_${VERSION}_amd64.deb"

log "rendering Homebrew formula"
"$ROOT/scripts/render-homebrew-formula.sh" "$VERSION" \
  "$ASSET_BASE/sootie-$VERSION-macos-arm64.tar.gz" \
  "$ROOT/dist/sootie-$VERSION-macos-arm64.tar.gz" \
  "$ASSET_BASE/sootie-$VERSION-macos-x64.tar.gz" \
  "$ROOT/dist/sootie-$VERSION-macos-x64.tar.gz"

if [[ "$PUSH" == "1" ]]; then
  log "publishing release asset branch $ASSETS_BRANCH"
  publish_assets_branch

  log "publishing apt branch $APT_BRANCH"
  publish_apt_branch

  log "publishing Homebrew tap"
  publish_homebrew_tap

  log "publishing git tag v$VERSION"
  publish_tag

  if [[ "$SKIP_PUBLIC_CHECK" == "0" ]]; then
    log "checking public distribution entrypoints"
    SOOTIE_REPO="$REPO" "$ROOT/scripts/check-distribution-entrypoints.sh"
  fi
else
  log "dry run complete; add --push to publish assets, apt metadata, tap, and tag"
fi
