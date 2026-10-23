# Skiff's probe of a Host. Read only: it changes nothing. It prints one JSON
# object on one line. $1: the skiffd socket name to look for in the runtime
# folder. $2, when set: a full socket path the person chose instead.
name=${1:-skiffd.sock}
want=$2
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g' | tr -d '\000-\037'; }
os=$(uname -s 2>/dev/null)
arch=$(uname -m 2>/dev/null)
# The same rule as skiff_core::socket::runtime_dir.
uid=$(id -u 2>/dev/null)
if [ -n "$XDG_RUNTIME_DIR" ] && [ -d "$XDG_RUNTIME_DIR" ]; then
  rt=$XDG_RUNTIME_DIR
elif [ -n "$uid" ] && [ -d "/run/user/$uid" ]; then
  rt=/run/user/$uid
elif [ "$os" = Darwin ] && d=$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null) && [ -n "$d" ]; then
  rt=$d
else
  rt=${TMPDIR:-/tmp}
fi
rt=${rt%/}
sock=${want:-$rt/skiff/$name}
live=false
[ -S "$sock" ] && live=true
# A skiff CLI to ask: the managed install first, then PATH, then cargo's.
cli=
for c in "$HOME/.local/share/skiff/server/current/skiff" "$(command -v skiff 2>/dev/null)" "$HOME/.cargo/bin/skiff"; do
  if [ -n "$c" ] && [ -x "$c" ]; then cli=$c; break; fi
done
status=null
if [ -n "$cli" ] && [ "$live" = true ]; then
  if command -v timeout >/dev/null 2>&1; then
    out=$(timeout 5 "$cli" --socket "$sock" status --json 2>/dev/null)
  else
    out=$("$cli" --socket "$sock" status --json 2>/dev/null)
  fi
  case $out in
    "{"*"}") status=$out ;;
  esac
fi
# The install Skiff manages, if any: its manifest names the version.
srv=$HOME/.local/share/skiff/server
managed=null
if [ -f "$srv/current/manifest.json" ] && [ -f "$srv/installed-by" ]; then
  mv_=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$srv/current/manifest.json" | head -n 1)
  mp_=$(sed -n 's/.*"protocol": *\([0-9]*\).*/\1/p' "$srv/current/manifest.json" | head -n 1)
  managed=$(printf '{"version":"%s","protocol":%s}' "$(esc "$mv_")" "${mp_:-0}")
fi
# How it keeps running: a systemd user manager and its unit, linger, or the
# fallback start's pid file.
systemd=false
unit_enabled=false
unit_active=false
host_unit_enabled=false
linger=
if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
  systemd=true
  systemctl --user -q is-enabled skiffd 2>/dev/null && unit_enabled=true
  systemctl --user -q is-active skiffd 2>/dev/null && unit_active=true
  systemctl --user -q is-enabled skiff-host 2>/dev/null && host_unit_enabled=true
  linger=$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null)
fi
fallback=false
if [ -f "$srv/skiffd.pid" ] && kill -0 "$(cat "$srv/skiffd.pid" 2>/dev/null)" 2>/dev/null; then fallback=true; fi
free_kb=$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2 {print $4}')
printf '{"os":"%s","arch":"%s","runtime_dir":"%s","socket":"%s","socket_live":%s,"cli":"%s","status":%s,"managed":%s,"systemd":%s,"unit_enabled":%s,"unit_active":%s,"host_unit_enabled":%s,"linger":"%s","fallback":%s,"free_kb":%s}\n' \
  "$(esc "$os")" "$(esc "$arch")" "$(esc "$rt")" "$(esc "$sock")" "$live" "$(esc "$cli")" "$status" "$managed" "$systemd" "$unit_enabled" "$unit_active" "$host_unit_enabled" "$(esc "$linger")" "$fallback" "${free_kb:-0}"
