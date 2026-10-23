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
printf '{"os":"%s","arch":"%s","runtime_dir":"%s","socket":"%s","socket_live":%s,"cli":"%s","status":%s}\n' \
  "$(esc "$os")" "$(esc "$arch")" "$(esc "$rt")" "$(esc "$sock")" "$live" "$(esc "$cli")" "$status"
