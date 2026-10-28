#!/bin/sh
# Writes the updater's latest.json to stdout from the signed packages under DIR.
# Usage: latest-json.sh DIR TAG BASE_URL
# Each package needs its .sig beside it. A package without one is left out.
set -eu

dir=$1 tag=$2 base=$3
version=${tag#v}
platforms='{}'

add() { # KEY PATTERN
  file=$(find "$dir" -name "$2" -type f | head -n 1)
  if [ -z "$file" ] || [ ! -f "$file.sig" ]; then
    echo "latest-json: no signed $2 for $1" >&2
    return
  fi
  platforms=$(printf '%s' "$platforms" | jq --arg key "$1" --arg url "$base/$(basename "$file")" \
    --arg sig "$(cat "$file.sig")" '. + {($key): {signature: $sig, url: $url}}')
}

add darwin-aarch64 '*.app.tar.gz'
add linux-x86_64-appimage '*.AppImage'
add linux-x86_64-deb '*.deb'
add linux-x86_64-rpm '*.rpm'

if [ "$platforms" = '{}' ]; then
  echo "latest-json: no signed package under $dir" >&2
  exit 1
fi

jq -n --arg version "$version" --arg notes "https://github.com/solutionscay/skiff/releases/tag/$tag" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $date, platforms: $platforms}'
