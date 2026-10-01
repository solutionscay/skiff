#!/bin/sh
set -eu

# GNOME Software looks up local .deb files by package name in its catalog.
# Keep the catalog in step with the upstream metadata and release history.
metadata_dir="$(dirname "$0")/../src-tauri/linux"
mkdir -p "$metadata_dir/generated"
{
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' '<components version="1.0" origin="skiff">'
  sed -e '1{/^<?xml /d;}' \
      -e '/<metadata_license>/d' \
      -e '/<id>com.solutionscay.skiff<\/id>/a\
  <pkgname>skiff</pkgname>\
  <icon type="stock">skiff-desktop</icon>' \
      "$metadata_dir/com.solutionscay.skiff.metainfo.xml"
  printf '%s\n' '</components>'
} > "$metadata_dir/generated/com.solutionscay.skiff.xml"
