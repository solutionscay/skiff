#!/usr/bin/env bash
# Bump the version, tag it, and push. The tag starts the Release workflow,
# which builds the Linux packages and attaches them to a draft release.
#
#   scripts/release.sh [major|minor|patch]   (default: minor)
#
# The version lives in the root Cargo.toml ([workspace.package]). Every crate
# and the Tauri app read it from there. package.json follows it.
set -euo pipefail

part="${1:-minor}"
cd "$(git rev-parse --show-toplevel)"

[[ "$part" =~ ^(major|minor|patch)$ ]] || { echo "usage: $0 [major|minor|patch]" >&2; exit 2; }
[[ "$(git branch --show-current)" == main ]] || { echo "Release from main." >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo "Commit or stash your changes first." >&2; exit 1; }
git fetch -q origin main
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] || { echo "main is not level with origin/main." >&2; exit 1; }

old="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)"
IFS=. read -r major minor patch <<<"$old"
case "$part" in
  major) new="$((major + 1)).0.0" ;;
  minor) new="$major.$((minor + 1)).0" ;;
  patch) new="$major.$minor.$((patch + 1))" ;;
esac
git rev-parse -q --verify "refs/tags/v$new" >/dev/null && { echo "Tag v$new exists." >&2; exit 1; }

read -r -p "Release v$old -> v$new? [y/N] " ok
[[ "$ok" == [yY] ]] || exit 1

sed -i "0,/^version = \"$old\"$/s//version = \"$new\"/" Cargo.toml
sed -i "0,/\"version\": \"$old\"/s//\"version\": \"$new\"/" apps/desktop/package.json
# Refresh the workspace entries in Cargo.lock. Dependencies stay as they are.
cargo metadata --format-version 1 >/dev/null

git add Cargo.toml Cargo.lock apps/desktop/package.json
git commit -q -m "Release v$new"
git tag -a "v$new" -m "v$new"
git push -q origin main "v$new"
echo "Pushed v$new. The Release workflow builds the packages into a draft release."
