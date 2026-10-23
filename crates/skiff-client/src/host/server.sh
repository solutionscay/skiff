# Skiff's managed install on a Linux server, in the account's home only. No
# sudo, no system folder. $1: install, start, linger, sessions or
# uninstall. install: $2 the version, $3 the SHA-256 of the bundle Skiff uploaded to
# ~/.local/share/skiff/server/.incoming.tar.gz, $4 the skiffd socket name.
# start: $2 the socket name. Each step prints key=value lines and ends with
# skiff-<mode>=<outcome>. It never stops or replaces a skiffd it did not
# start: Skiff only runs it after the check found no other one.
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
  if [ -S "$sock" ]; then echo "skiff-start=running"; else echo "skiff-start=stopped"; fi
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

# Remove and uninstall. A unit or a pid is Skiff's only when it runs from
# the managed folder; anything else is left as it is.
ours_unit() { [ -f "$1" ] && grep -q '^ExecStart=%h/.local/share/skiff/server/current/' "$1"; }
ours_pid() { [ -n "$1" ] && kill -0 "$1" 2>/dev/null && case "$(ps -o args= -p "$1" 2>/dev/null)" in "$d"/*) true ;; *) false ;; esac; }

# The managed skiffd's sessions, as `skiff ls --json` on one line. $1: the
# socket name. managed=no: Skiff did not install Skiff here.
sessions() {
  [ -f "$d/installed-by" ] || { echo managed=no; echo "skiff-sessions=done"; return; }
  echo managed=yes
  up=no
  if has_systemd && ours_unit "$HOME/.config/systemd/user/skiffd.service" && systemctl --user -q is-active skiffd; then up=yes
  elif ours_pid "$(cat "$d/skiffd.pid" 2>/dev/null)"; then up=yes; fi
  if [ $up = yes ] && [ -x "$d/current/skiff" ]; then
    printf 'sessions='; "$d/current/skiff" --socket "$rt/skiff/$1" ls --json 2>/dev/null | tr -d '\n'; echo
  fi
  echo "skiff-sessions=done"
}

# Stops and disables Skiff's services, then deletes the managed folder,
# Skiff's unit files and the skiff link if it is Skiff's. Projects, session
# data and local-host-id stay.
uninstall() {
  [ -f "$d/installed-by" ] || { echo "skiff-uninstall=unmanaged"; return; }
  u=$HOME/.config/systemd/user
  if has_systemd; then
    for s in skiff-host skiffd; do
      if ours_unit "$u/$s.service"; then
        systemctl --user disable --now -q "$s" 2>/dev/null
        rm -f "$u/$s.service"
      fi
    done
    systemctl --user daemon-reload 2>/dev/null
  fi
  for s in skiff-host skiffd; do
    pid=$(cat "$d/$s.pid" 2>/dev/null)
    ours_pid "$pid" || continue
    kill "$pid" 2>/dev/null
    n=0
    while kill -0 "$pid" 2>/dev/null && [ $n -lt 50 ]; do sleep 0.1; n=$((n + 1)); done
    kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
  done
  l=$HOME/.local/bin/skiff
  [ -L "$l" ] && [ "$(readlink "$l")" = "$d/current/skiff" ] && rm -f "$l"
  rm -rf "${d:?}"
  echo "skiff-uninstall=done"
}

case $mode in
  install) mkdir -p "$d"; install "$2" "$3"; start "$4" ;;
  start) start "$2" ;;
  linger) linger; echo "skiff-linger=done" ;;
  sessions) sessions "$2" ;;
  uninstall) uninstall ;;
  *) echo "skiff-$mode=unknown" ;;
esac
