#!/bin/sh
# Builds skiffd and the skiff CLI for release and puts them where Tauri bundles
# them: next to the app binary, from src-tauri/binaries/<name>-<target triple>.
# TARGET picks a cross target; the host is the default.
set -eu
cd "$(dirname "$0")/.."
triple="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
if [ -n "${TARGET:-}" ]; then
  cargo build --release -p skiffd -p skiff-cli --target "$TARGET"
  dir="../../target/$TARGET/release"
else
  cargo build --release -p skiffd -p skiff-cli
  dir="../../target/release"
fi
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac
mkdir -p src-tauri/binaries
for bin in skiffd skiff; do
  cp "$dir/$bin$ext" "src-tauri/binaries/$bin-$triple$ext"
done
