#!/bin/sh
# Builds skiffd for release and puts it where Tauri bundles it: next to the
# app binary as skiffd, from src-tauri/binaries/skiffd-<target triple>.
# TARGET picks a cross target; the host is the default.
set -eu
cd "$(dirname "$0")/.."
triple="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
if [ -n "${TARGET:-}" ]; then
  cargo build --release -p skiffd --target "$TARGET"
  out="../../target/$TARGET/release/skiffd"
else
  cargo build --release -p skiffd
  out="../../target/release/skiffd"
fi
mkdir -p src-tauri/binaries
cp "$out" "src-tauri/binaries/skiffd-$triple"
