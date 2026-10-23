# Skiff's managed install on a Linux server, in the account's home only. No
# sudo, no system folder. $1: install, start or linger. install: $2 the
# version, $3 the SHA-256 of the bundle Skiff uploaded to
# ~/.local/share/skiff/server/.incoming.tar.gz, $4 the skiffd socket name.
# start: $2 the socket name. Each step prints key=value lines and ends with
# skiff-<mode>=<outcome>. It never stops or replaces a skiffd it did not
# start: Skiff only runs it after the check found no other one. skiff-host
# (projects) runs beside the managed skiffd, and restarts after an install
# that changed the version.
set -u
mode=$1
d=$HOME/.local/share/skiff/server
umask 077
uid=$(id -u)
if [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -d "$XDG_RUNTIME_DIR" ]; then rt=$XDG_RUNTIME_DIR
elif [ -d "/run/user/$uid" ]; then rt=/run/user/$uid
else rt=${TMPDIR:-/tmp}; fi
rt=${rt%/}
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d ' ' -f 1; else shasum -a 256 "$1" | cut -d ' ' -f 1; fi; }
has_systemd() { command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; }

linger() {
  me=$(id -un)
  if [ "$(loginctl show-user "$me" -p Linger --value 2>/dev/null)" = yes ]; then echo linger=yes
  elif loginctl enable-linger "$me" >/dev/null 2>&1 && [ "$(loginctl show-user "$me" -p Linger --value 2>/dev/null)" = yes ]; then echo linger=yes
  else echo linger=refused; fi
}

# Starts the managed skiffd if it does not run, and waits for its socket.
start() {
  sock=$rt/skiff/$1
  [ -x "$d/current/skiffd" ] || { echo "skiff-start=missing"; return; }
  if has_systemd; then
    u=$HOME/.config/systemd/user
    mkdir -p "$u"
    if ! cmp -s "$d/current/systemd/skiffd.service" "$u/skiffd.service"; then
      cp "$d/current/systemd/skiffd.service" "$u/skiffd.service"
      systemctl --user daemon-reload
    fi
    systemctl --user enable -q skiffd 2>/dev/null
    systemctl --user -q is-active skiffd || systemctl --user start skiffd
    echo how=systemd
    linger
    host_unit "$1"
  else
    # Without a user manager: it runs until the server restarts.
    pid=$(cat "$d/skiffd.pid" 2>/dev/null)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then :
    elif command -v setsid >/dev/null 2>&1; then
      setsid nohup "$d/current/skiffd" </dev/null >/dev/null 2>&1 &
      echo $! > "$d/skiffd.pid"
    else
      nohup "$d/current/skiffd" </dev/null >/dev/null 2>&1 &
      echo $! > "$d/skiffd.pid"
    fi
    echo how=fallback
  fi
  n=0
  while [ ! -S "$sock" ] && [ $n -lt 50 ]; do sleep 0.1; n=$((n + 1)); done
  has_systemd || host_fallback "$sock"
  hs=$rt/skiff/${1%.sock}-host.sock
  n=0
  while [ ! -S "$hs" ] && [ $n -lt 50 ] && host_alive; do sleep 0.1; n=$((n + 1)); done
  if [ -S "$hs" ] && host_alive; then echo host=running; else echo host=stopped; fi
  if [ -S "$sock" ]; then echo "skiff-start=running"; else echo "skiff-start=stopped"; fi
}

# skiff-host's user unit, for the skiffd socket named $1: enabled, started,
# restarted by systemd when it stops, and after an install that changed the
# version ($restart_host).
host_unit() {
  u=$HOME/.config/systemd/user
  sed "s#/skiffd\.sock#/$1#" "$d/current/systemd/skiff-host.service" > "$u/.skiff-host.service.new"
  if cmp -s "$u/.skiff-host.service.new" "$u/skiff-host.service"; then rm -f "$u/.skiff-host.service.new"
  else mv "$u/.skiff-host.service.new" "$u/skiff-host.service"; systemctl --user daemon-reload; fi
  systemctl --user enable -q skiff-host 2>/dev/null
  if [ "${restart_host:-}" = yes ]; then systemctl --user restart skiff-host
  else systemctl --user -q is-active skiff-host || systemctl --user start skiff-host; fi
}

# The pid file's skiff-host runs. Never another process that took its pid.
host_pid() {
  p=$(cat "$d/skiff-host.pid" 2>/dev/null)
  [ -n "$p" ] && kill -0 "$p" 2>/dev/null && tr '\000' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -q skiff-host && echo "$p"
}
host_alive() { has_systemd && systemctl --user -q is-active skiff-host || [ -n "$(host_pid)" ]; }

# Without a user manager: skiff-host for the skiffd at $1, until the server
# restarts. After an install that changed the version, the old one stops first.
host_fallback() {
  p=$(host_pid)
  if [ -n "$p" ] && [ "${restart_host:-}" = yes ]; then
    kill "$p" 2>/dev/null
    n=0
    while kill -0 "$p" 2>/dev/null && [ $n -lt 50 ]; do sleep 0.1; n=$((n + 1)); done
    p=
  fi
  [ -n "$p" ] && return
  if command -v setsid >/dev/null 2>&1; then
    setsid nohup "$d/current/skiff-host" --session-socket "$1" </dev/null >/dev/null 2>&1 &
  else
    nohup "$d/current/skiff-host" --session-socket "$1" </dev/null >/dev/null 2>&1 &
  fi
  echo $! > "$d/skiff-host.pid"
}

install() {
  v=$1 want=$2
  in=$d/.incoming.tar.gz
  [ -f "$in" ] || { echo "skiff-install=no-upload"; return; }
  if [ "$(sha "$in")" != "$want" ]; then rm -f "$in"; echo "skiff-install=bad-hash"; return; fi
  prev=$(readlink "$d/current" 2>/dev/null)
  if [ "$prev" = "$v" ] && [ -x "$d/$v/skiffd" ]; then
    echo changed=no
  else
    tmp=$(mktemp -d "$d/.unpack.XXXXXX") || { echo "skiff-install=no-room"; return; }
    if ! tar -xzf "$in" -C "$tmp" 2>/dev/null; then rm -rf "$tmp" "$in"; echo "skiff-install=no-room"; return; fi
    src=$(find "$tmp" -mindepth 1 -maxdepth 1 -type d -name "skiff-server-$v-*" | head -n 1)
    if [ -z "$src" ] || ! grep -q "\"version\": *\"$v\"" "$src/manifest.json" 2>/dev/null; then
      rm -rf "$tmp" "$in"; echo "skiff-install=bad-bundle"; return
    fi
    chmod 755 "$src" "$src/skiffd" "$src/skiff-host" "$src/skiff"
    rm -rf "${d:?}/$v"
    mv "$src" "$d/$v"
    rm -rf "$tmp"
    # A half-unpacked version never becomes current: the link moves last.
    ln -sfn "$v" "$d/current"
    echo app > "$d/installed-by"
    # Keep the one before, for a rollback.
    for x in "$d"/*; do
      n=${x##*/}
      [ -d "$x" ] && [ ! -L "$x" ] || continue
      [ "$n" = "$v" ] || [ "$n" = "$prev" ] || rm -rf "$x"
    done
    echo changed=yes
    restart_host=yes
  fi
  rm -f "$in"
  # The skiff CLI for agents on the server, only where the name is free or ours.
  mkdir -p "$HOME/.local/bin"
  l=$HOME/.local/bin/skiff
  if [ ! -e "$l" ] && [ ! -L "$l" ]; then ln -s "$d/current/skiff" "$l"; echo cli=linked
  elif [ "$(readlink "$l" 2>/dev/null)" = "$d/current/skiff" ]; then echo cli=linked
  else echo cli=taken; fi
  echo "skiff-install=done"
}

case $mode in
  install) mkdir -p "$d"; install "$2" "$3"; start "$4" ;;
  start) start "$2" ;;
  linger) linger; echo "skiff-linger=done" ;;
  *) echo "skiff-$mode=unknown" ;;
esac
