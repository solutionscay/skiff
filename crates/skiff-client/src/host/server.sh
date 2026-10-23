# Skiff's managed install on a Linux or macOS server, in the account's home
# only. No sudo, no system folder. $1: install, update, start, linger,
# sessions or uninstall. install: $2 the version, $3 the SHA-256 of the
# bundle Skiff uploaded to ~/.local/share/skiff/server/.incoming-<sha256>.tar.gz,
# $4 the skiffd socket name. update: the same, and $5 yes when the person
# agreed that sessions end if skiffd cannot hand them over. start: $2 the
# socket name. Each step prints key=value lines and ends with
# skiff-<mode>=<outcome>. It never stops or replaces a skiffd it did not
# start: Skiff only runs it after the check found no other one. skiff-host
# (projects) runs beside the managed skiffd, and restarts after an install
# that changed the version.
set -u
mode=$1
d=$HOME/.local/share/skiff/server
umask 077
uid=$(id -u)
os=$(uname -s)
if [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -d "$XDG_RUNTIME_DIR" ]; then rt=$XDG_RUNTIME_DIR
elif [ -d "/run/user/$uid" ]; then rt=/run/user/$uid
elif [ "$os" = Darwin ] && g=$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null) && [ -n "$g" ]; then rt=$g
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

# macOS: LaunchAgents. launchd starts skiffd at login and after a crash; a
# second skiffd on a live socket exits with an error, so it stays down.
# skiff-host holds no sessions: launchd starts it again whenever it stops,
# as Restart=always does on Linux.
label=com.solutionscay.skiff.skiffd
hlabel=com.solutionscay.skiff.skiff-host
agents=$HOME/Library/LaunchAgents
xml() { printf '%s' "$1" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g'; }
# $1 the label, $2 the KeepAlive value, then the program and its arguments.
plist() {
  l=$1 keep=$2
  shift 2
  cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$l</string>
  <key>ProgramArguments</key>
  <array>
EOF
  for x in "$@"; do printf '    <string>%s</string>\n' "$(xml "$x")"; done
  cat <<EOF
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  $keep
  <key>ProcessType</key>
  <string>Interactive</string>
</dict>
</plist>
EOF
}
# The launchd domain that has agent $1 loaded, if any.
loaded() {
  for x in "gui/$uid" "user/$uid"; do
    if launchctl print "$x/$1" >/dev/null 2>&1; then echo "$x"; return; fi
  done
}
# Writes and loads agent $1 (KeepAlive $2, program and arguments after).
# $kick: -k restarts one that runs, for skiff-host after a new version.
agent() {
  l=$1
  mkdir -p "$agents"
  # A changed file waits for the next load: a reload ends every session.
  plist "$@" > "$agents/.$l.plist" && mv "$agents/.$l.plist" "$agents/$l.plist"
  on=$(loaded "$l")
  if [ -n "$on" ]; then
    # Starts it only if it does not run, or restarts it with -k.
    launchctl kickstart ${kick:-} "$on/$l" >/dev/null 2>&1
  else
    # Homebrew's rule: the GUI domain when this account owns the screen,
    # else the user domain, which an ssh login reaches and which keeps
    # running after logout.
    on=user/$uid
    [ "$(ls -ln /dev/console 2>/dev/null | awk '{print $3}')" = "$uid" ] && on=gui/$uid
    launchctl bootstrap "$on" "$agents/$l.plist" >/dev/null 2>&1
  fi
}

# Starts the managed skiffd if it does not run, and waits for its socket.
start() {
  sock=$rt/skiff/$1
  [ -x "$d/current/skiffd" ] || { echo "skiff-start=missing"; return; }
  if [ "$os" = Darwin ]; then
    agent "$label" '<dict>
    <key>Crashed</key>
    <true/>
  </dict>' "$d/current/skiffd"
    echo how=launchd
    echo "domain=${on%%/*}"
  elif has_systemd; then
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
  if [ "$os" = Darwin ]; then
    kick=${restart_host:+-k} agent "$hlabel" '<true/>' "$d/current/skiff-host" --session-socket "$sock"
  elif ! has_systemd; then
    host_fallback "$sock"
  fi
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
host_alive() {
  if [ "$os" = Darwin ]; then
    on=$(loaded "$hlabel")
    [ -n "$on" ] && launchctl print "$on/$hlabel" 2>/dev/null | grep -q 'state = running'
  else
    has_systemd && systemctl --user -q is-active skiff-host || [ -n "$(host_pid)" ]
  fi
}

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
  in=$d/.incoming-$want.tar.gz
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

# Remove and uninstall. A unit or a pid is Skiff's only when it runs from
# the managed folder; anything else is left as it is.
ours_unit() { [ -f "$1" ] && grep -q '^ExecStart=%h/.local/share/skiff/server/current/' "$1"; }
ours_agent() { [ -f "$agents/$1.plist" ] && grep -qF "<string>$(xml "$d/current/")" "$agents/$1.plist"; }
ours_pid() { [ -n "$1" ] && kill -0 "$1" 2>/dev/null && case "$(ps -o args= -p "$1" 2>/dev/null)" in "$d"/*) true ;; *) false ;; esac; }

# The managed skiffd's sessions, as `skiff ls --json` on one line. $1: the
# socket name. managed=no: Skiff did not install Skiff here.
sessions() {
  [ -f "$d/installed-by" ] || { echo managed=no; echo "skiff-sessions=done"; return; }
  echo managed=yes
  up=no
  if [ "$os" = Darwin ] && ours_agent "$label" && [ -n "$(loaded "$label")" ]; then up=yes
  elif has_systemd && ours_unit "$HOME/.config/systemd/user/skiffd.service" && systemctl --user -q is-active skiffd; then up=yes
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
  if [ "$os" = Darwin ]; then
    for l in "$hlabel" "$label"; do
      ours_agent "$l" || continue
      on=$(loaded "$l")
      [ -n "$on" ] && launchctl bootout "$on/$l" >/dev/null 2>&1
      rm -f "${agents:?}/${l:?}.plist"
    done
  elif has_systemd; then
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

# One install or update at a time. A second one waits for the first, then
# finds its version there. A lock whose holder is gone is taken over.
lock() {
  n=0
  until mkdir "$d/.lock" 2>/dev/null; do
    p=$(cat "$d/.lock/pid" 2>/dev/null)
    if [ -n "$p" ] && ! kill -0 "$p" 2>/dev/null; then rm -rf "$d/.lock"; continue; fi
    [ $n -ge 1500 ] && { echo "skiff-$mode=busy"; exit 0; }
    sleep 0.1; n=$((n + 1))
  done
  echo $$ > "$d/.lock/pid"
  trap 'rm -rf "$d/.lock"' EXIT
}

# True when version $1 is newer than $2, by major.minor.patch.
newer() {
  awk -v a="$1" -v b="$2" 'BEGIN { split(a, x, /[.+-]/); split(b, y, /[.+-]/)
    for (i = 1; i <= 3; i++) { if (x[i] + 0 > y[i] + 0) exit 0; if (x[i] + 0 < y[i] + 0) exit 1 } exit 1 }'
}

# The running skiffd's pid, from `skiff status --json`.
daemon() { "$1" --socket "$sock" status --json 2>/dev/null | sed -n 's/.*"skiffd":{[^}]*"pid":\([0-9]*\).*/\1/p'; }

# Moves the managed install up to $1, never down. skiffd reloads onto the
# new binary with its sessions (same pid, same screens). If it cannot hand
# them over, nothing changes and the live sessions are listed, unless $4 is
# yes: then skiffd restarts on the new version. A failed reload switches
# `current` back and keeps the old skiffd running.
update() {
  v=$1 want=$2 sock=$rt/skiff/$3 agreed=$4
  old=$(readlink "$d/current" 2>/dev/null)
  echo "from=$old"
  if [ -n "$old" ] && newer "$old" "$v"; then rm -f "$d/.incoming-$want.tar.gz"; echo "skiff-update=newer"; return; fi
  out=$(install "$v" "$want")
  echo "$out"
  case $out in *skiff-install=done*) ;; *) echo "skiff-update=failed"; return ;; esac
  # That version is there already: another computer updated it first.
  case $out in *changed=no*) echo "skiff-update=same"; return ;; esac
  [ -n "$old" ] || old=$v
  # The version moved: the start after this step restarts skiff-host on it,
  # with the restart an install uses (systemd, the fallback, or launchd).
  # A path below that moves `current` back clears it.
  restart_host=yes
  cli=$d/$old/skiff
  [ -x "$cli" ] || cli=$d/$v/skiff
  pid=$(daemon "$cli")
  if [ -z "$pid" ]; then
    # skiffd does not run: the next start runs the new version.
    echo "skiff-update=done"; return
  fi
  if "$cli" --socket "$sock" daemon reload --binary "$d/$v/skiffd" >/dev/null 2>&1; then
    echo "reload=kept"; echo "skiff-update=done"; return
  fi
  now=$(daemon "$d/$v/skiff")
  if [ -n "$now" ] && [ "$now" != "$pid" ]; then echo "skiff-update=lost"; return; fi
  if [ -z "$now" ]; then
    # It did not come back: the old version starts again.
    ln -sfn "$old" "$d/current"; restart_host=; start "$3" >/dev/null; echo "skiff-update=failed"; return
  fi
  # The old skiffd runs on, untouched: it cannot hand its sessions over.
  live=$("$cli" --socket "$sock" ls --json 2>/dev/null | tr -d '\n')
  if [ "$agreed" != yes ] && ! printf '%s' "$live" | grep -Eq '"state": *"(working|idle|waiting)"'; then agreed=yes; fi
  if [ "$agreed" != yes ]; then
    ln -sfn "$old" "$d/current"
    restart_host=
    echo "sessions=$live"; echo "skiff-update=no-handover"; return
  fi
  "$cli" --socket "$sock" daemon stop >/dev/null 2>&1
  if has_systemd; then systemctl --user restart skiffd
  elif [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then kill "$pid"; sleep 1; fi
  echo "reload=restarted"; echo "skiff-update=done"
}

case $mode in
  install) mkdir -p "$d"; lock; install "$2" "$3"; start "$4" ;;
  update) mkdir -p "$d"; lock; update "$2" "$3" "$4" "${5:-no}"; start "$4" ;;
  start) start "$2" ;;
  linger) linger; echo "skiff-linger=done" ;;
  sessions) sessions "$2" ;;
  uninstall) uninstall ;;
  *) echo "skiff-$mode=unknown" ;;
esac
