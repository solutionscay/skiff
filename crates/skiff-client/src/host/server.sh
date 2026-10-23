# Everything Skiff runs on a server, as the account that owns the sessions:
# no sudo, no system folder. The app sends this file on stdin, with a mode:
#   probe [socket name] [socket path]  read only: one JSON line for the check list
#   install-key <key>, fix-key <key>   this computer's public key into authorized_keys
#   install <version> <sha256> <socket name>, update <version> <sha256> <socket name> <yes|no>,
#   start <socket name>, linger, sessions <socket name>, uninstall
# The managed install is ~/.local/share/skiff/server. install: $3 is the
# SHA-256 of the bundle Skiff uploaded to .incoming-<sha256>.tar.gz there;
# update: the same, and $5 yes when the person agreed that sessions end if
# skiffd cannot hand them over. Each step prints key=value lines and ends
# with skiff-<mode>=<outcome>. It never stops or replaces a skiffd it did
# not start: Skiff only runs it after the check found no other one.
# skiff-host (projects) runs beside the managed skiffd, and restarts after
# an install that changed the version.
#
# The same script is install.sh, for a server with only a shell. With no
# mode it installs or updates the release written in below, and prints in
# plain words what it did; --yes ends sessions that cannot be handed over;
# --uninstall removes the install. The release job writes in the version,
# the SHA-256 of each bundle and the public key of their signatures.
set -u
release=''
sums=''
pubkey=''
mode=${1:-}
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

# Read only, for the check list: one JSON object on one line. $1: the
# skiffd socket name to look for in the runtime folder. $2, when set: a
# full socket path the person chose instead.
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g' | tr -d '\000-\037'; }
probe() {
  sock=${2:-$rt/skiff/${1:-skiffd.sock}}
  arch=$(uname -m)
  live=false
  [ -S "$sock" ] && live=true
  # `skiff status --json` from the skiffd there: the managed CLI first,
  # then PATH, then cargo's.
  status=null
  if [ $live = true ]; then
    s=$(status_of "$d/current/skiff" "$(command -v skiff 2>/dev/null)" "$HOME/.cargo/bin/skiff")
    [ -n "$s" ] && status=$s
  fi
  # The install Skiff manages, if any: its manifest names the version.
  managed=null
  if [ -f "$d/current/manifest.json" ] && [ -f "$d/installed-by" ]; then
    mv_=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$d/current/manifest.json" | head -n 1)
    mp_=$(sed -n 's/.*"protocol": *\([0-9]*\).*/\1/p' "$d/current/manifest.json" | head -n 1)
    managed=$(printf '{"version":"%s","protocol":%s}' "$(esc "$mv_")" "${mp_:-0}")
  fi
  # The Skiff app's own skiffd (a .deb, or Skiff.app): it runs while the
  # app runs there, and Skiff never installs a second one beside it.
  app=
  for a in /usr/bin/skiffd /Applications/Skiff.app/Contents/MacOS/skiffd "$HOME/Applications/Skiff.app/Contents/MacOS/skiffd"; do
    if [ -x "$a" ]; then app=$a; break; fi
  done
  # How it keeps running: a systemd user manager, its units and linger;
  # the LaunchAgents; or the fallback start's pid file.
  systemd=false unit_enabled=false unit_active=false host_unit_enabled=false linger=
  if has_systemd; then
    systemd=true
    systemctl --user -q is-enabled skiffd 2>/dev/null && unit_enabled=true
    systemctl --user -q is-active skiffd 2>/dev/null && unit_active=true
    systemctl --user -q is-enabled skiff-host 2>/dev/null && host_unit_enabled=true
    linger=$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null)
  elif [ "$os" = Darwin ]; then
    [ -f "$agents/$label.plist" ] && unit_enabled=true
    [ -f "$agents/$hlabel.plist" ] && host_unit_enabled=true
    [ -n "$(loaded "$label")" ] && unit_active=true
  fi
  fallback=false
  if [ -f "$d/skiffd.pid" ] && kill -0 "$(cat "$d/skiffd.pid" 2>/dev/null)" 2>/dev/null; then fallback=true; fi
  free_kb=$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2 {print $4}')
  printf '{"os":"%s","arch":"%s","runtime_dir":"%s","socket":"%s","socket_live":%s,"status":%s,"managed":%s,"app":"%s","systemd":%s,"unit_enabled":%s,"unit_active":%s,"host_unit_enabled":%s,"linger":"%s","fallback":%s,"free_kb":%s}\n' \
    "$(esc "$os")" "$(esc "$arch")" "$(esc "$rt")" "$(esc "$sock")" "$live" "$status" "$managed" "$(esc "$app")" "$systemd" "$unit_enabled" "$unit_active" "$host_unit_enabled" "$(esc "$linger")" "$fallback" "${free_kb:-0}"
}

# This computer's public key line $1, checked by Skiff against a strict
# pattern, into ~/.ssh/authorized_keys once: only when that exact line is
# not there. Nothing else changes, except the folder and file modes of
# what the account owns.
owner() { ls -nd "$1" 2>/dev/null | awk '{print $3}'; }
install_key() {
  key=$1
  [ -d "$HOME/.ssh" ] || mkdir -p "$HOME/.ssh"
  ak=$HOME/.ssh/authorized_keys
  added=no
  if [ -f "$ak" ] && grep -qxF -- "$key" "$ak" 2>/dev/null; then
    :
  elif [ ! -e "$ak" ] || [ -w "$ak" ]; then
    # A last line with no newline gets one first.
    if [ -s "$ak" ] && [ -n "$(tail -c 1 "$ak")" ]; then printf '\n' >> "$ak"; fi
    printf '%s\n' "$key" >> "$ak" && added=yes
  fi
  [ "$(owner "$HOME/.ssh")" = "$uid" ] && chmod 700 "$HOME/.ssh" && dir=yes || dir=no
  [ "$(owner "$ak")" = "$uid" ] && chmod 600 "$ak" && file=yes || file=no
  echo "added=$added"
  echo "ssh_dir_own=$dir"
  echo "file_own=$file"
  echo "skiff-install-key=done"
}

# ~/.ssh/authorized_keys belongs to another account, so sshd ignores it.
# The account owns ~/.ssh, so it can rename the file aside and write a new
# one with key $1. The old file stays, renamed.
fix_key() {
  key=$1
  [ "$(owner "$HOME/.ssh")" = "$uid" ] || { echo "skiff-fix-key=not-own"; return; }
  ak=$HOME/.ssh/authorized_keys
  if [ -e "$ak" ]; then mv -- "$ak" "$ak.skiff-$(date +%Y%m%d%H%M%S)" || return; fi
  printf '%s\n' "$key" > "$ak" && chmod 600 "$ak" && chmod 700 "$HOME/.ssh" && echo "skiff-fix-key=done"
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
    [ -f "$src/install.sh" ] && chmod 755 "$src/install.sh"
    rm -rf "${d:?}/${v:?}"
    mv "$src" "$d/$v"
    rm -rf "$tmp"
    # A half-unpacked version never becomes current: the link moves last.
    ln -sfn "$v" "$d/current"
    echo "${by:-app}" > "$d/installed-by"
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
    if [ $n -ge 1500 ]; then
      if [ "$mode" = headless ]; then echo "Skiff is being installed on this server by another run. Try again in a minute."; exit 1; fi
      echo "skiff-$mode=busy"; exit 0
    fi
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

# `skiff status --json` from the skiffd at $sock, by the first CLI in $@
# that is there and answers with one. Empty: none answers.
status_of() {
  for c in "$@"; do
    [ -n "$c" ] && [ -x "$c" ] || continue
    if command -v timeout >/dev/null 2>&1; then s=$(timeout 5 "$c" --socket "$sock" status --json 2>/dev/null)
    else s=$("$c" --socket "$sock" status --json 2>/dev/null); fi
    case $s in "{"*"}") printf '%s' "$s"; return ;; esac
  done
}
# Field $2 (pid, version) of the skiffd object in status $1. Empty: no skiffd.
daemon_field() { printf '%s' "$1" | sed -n "s/.*\"skiffd\":{[^}]*\"$2\":\"\{0,1\}\([^\",}]*\).*/\1/p"; }

# The skiffd at $pid runs version $v: by its binary's folder (Linux), or
# by the version it reports. $have: what it reports.
runs() {
  [ "$have" = "$v" ] && return 0
  case $(readlink "/proc/$pid/exe" 2>/dev/null) in "$d/$v/"*) return 0 ;; *) return 1 ;; esac
}

# Moves the managed install up to $1, never down. skiffd reloads onto the
# new binary with its sessions (same pid, same screens). If it cannot hand
# them over, nothing changes and the live sessions are listed, unless $4 is
# yes: then skiffd restarts on the new version. A failed reload switches
# `current` back and keeps the old skiffd running. A run that stopped after
# it moved `current` but before the reload is finished by the next one: the
# skiffd that runs decides, not the link.
update() {
  v=$1 want=$2 sock=$rt/skiff/$3 agreed=$4
  old=$(readlink "$d/current" 2>/dev/null)
  if [ -n "$old" ] && newer "$old" "$v"; then rm -f "$d/.incoming-$want.tar.gz"; echo "from=$old"; echo "skiff-update=newer"; return; fi
  out=$(install "$v" "$want")
  echo "$out"
  case $out in *skiff-install=done*) ;; *) echo "skiff-update=failed"; return ;; esac
  s=$(status_of "$d/$old/skiff" "$d/$v/skiff")
  pid=$(daemon_field "$s" pid)
  have=$(daemon_field "$s" version)
  from=$old
  if [ "$old" = "$v" ] && [ -n "$pid" ] && ! runs; then from=$have; fi
  echo "from=$from"
  # That version runs, or waits for the start after this step: another
  # computer updated it first.
  if [ "$old" = "$v" ] && { [ -z "$pid" ] || runs; }; then echo "skiff-update=same"; return; fi
  # The version moves: the start after this step restarts skiff-host on
  # it, with the restart an install uses (systemd, the fallback, or
  # launchd). A path below that moves `current` back clears it.
  restart_host=yes
  # skiffd does not run: the next start runs the new version.
  if [ -z "$pid" ]; then echo "skiff-update=done"; return; fi
  cli=$d/$from/skiff
  [ -x "$cli" ] || cli=$d/$v/skiff
  if "$cli" --socket "$sock" daemon reload --binary "$d/$v/skiffd" >/dev/null 2>&1; then
    echo "reload=kept"; echo "skiff-update=done"; return
  fi
  now=$(daemon_field "$(status_of "$d/$v/skiff" "$cli")" pid)
  if [ -n "$now" ] && [ "$now" != "$pid" ]; then echo "skiff-update=lost"; return; fi
  # Where `current` goes back to: the version that runs, while its folder is there.
  back=$from
  [ -x "$d/$back/skiffd" ] || back=$old
  if [ -z "$now" ]; then
    # It did not come back: the old version starts again.
    ln -sfn "$back" "$d/current"; restart_host=; start "$3" >/dev/null; echo "skiff-update=failed"; return
  fi
  # The old skiffd runs on, untouched: it cannot hand its sessions over.
  live=$("$cli" --socket "$sock" ls --json 2>/dev/null | tr -d '\n')
  if [ "$agreed" != yes ] && ! printf '%s' "$live" | grep -Eq '"state": *"(working|idle|waiting)"'; then agreed=yes; fi
  if [ "$agreed" != yes ]; then
    ln -sfn "$back" "$d/current"
    restart_host=
    echo "sessions=$live"; echo "skiff-update=no-handover"; return
  fi
  "$cli" --socket "$sock" daemon stop >/dev/null 2>&1
  if has_systemd; then systemctl --user restart skiffd
  elif kill -0 "$pid" 2>/dev/null; then kill "$pid"; sleep 1; fi
  echo "reload=restarted"; echo "skiff-update=done"
}

# install.sh on the server's own shell: the words are for a person.
say() { printf '%s\n' "$*"; }
stop() { say "$*" >&2; exit 1; }
val() { printf '%s\n' "$1" | sed -n "s/^$2=//p" | tail -n 1; }

# A download to $2, by curl or wget. Nothing reads the script's stdin.
fetch() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL --proto =https -o "$2" "$1" </dev/null
  elif command -v wget >/dev/null 2>&1; then wget -q -O "$2" "$1" </dev/null
  else return 1; fi
}

# The sessions skiffd runs now, one per line, as `skiff ls` shows them.
live_list() { "$d/current/skiff" --socket "$1" ls 2>/dev/null </dev/null | sed 's/^/  /'; }

live_count() {
  "$d/current/skiff" --socket "$1" ls --json 2>/dev/null </dev/null | grep -Ec '"state": *"(working|idle|waiting)"'
}

# Yes from the person: --yes, or "yes" typed on the terminal.
agree() {
  [ "$yes" = yes ] && return 0
  [ -t 2 ] && [ -r /dev/tty ] || return 1
  printf '%s' "$1 Type yes to go on: " >&2
  read -r a </dev/tty || return 1
  [ "$a" = yes ]
}

# --uninstall on the server's shell: the person agrees to end live sessions,
# then the one uninstall the app uses runs (systemd, the fallback, launchd).
headless_uninstall() {
  sock=$rt/skiff/skiffd.sock
  [ -e "$d/current" ] || stop "Skiff is not installed in ~/.local/share/skiff/server. Nothing changed."
  n=$(live_count "$sock")
  if [ "${n:-0}" -gt 0 ]; then
    say "Uninstall ends these sessions in Skiff on this server:"
    live_list "$sock"
    agree "Save their work first." || stop "Nothing changed. Run it again with --yes to end them."
  fi
  case $(uninstall) in
    *skiff-uninstall=done*) ;;
    *skiff-uninstall=unmanaged*) stop "Skiff did not install the Skiff in ~/.local/share/skiff/server. Nothing changed." ;;
    *) stop "The uninstall did not finish. Run it again." ;;
  esac
  say "Skiff is uninstalled from this server. Its Host id stays, so a later install is the same Host in the app."
}

headless() {
  yes=no un=no
  for a in "$@"; do
    case $a in
      --yes) yes=yes ;;
      --uninstall) un=yes ;;
      *) stop "install.sh knows only --yes and --uninstall, not $a." ;;
    esac
  done
  [ "$un" = yes ] && { headless_uninstall; return; }
  os=$(uname -s) arch=$(uname -m)
  case $arch in amd64) arch=x86_64 ;; arm64) arch=aarch64 ;; esac
  case $os/$arch in
    Linux/x86_64 | Linux/aarch64) bos=linux shown=Linux ;;
    Darwin/x86_64 | Darwin/aarch64) bos=macos shown=macOS ;;
    *) stop "This server runs $os on $arch. Skiff runs on Linux and macOS, on x86_64 and ARM64." ;;
  esac
  [ -n "$release" ] || stop "This install.sh names no release. Download it from a Skiff release."
  name=skiff-server-$release-$bos-$arch.tar.gz
  want=$(printf '%s\n' "$sums" | awk -v n="$name" '$2 == n { print $1 }')
  [ -n "$want" ] || stop "Skiff $release has no bundle for $shown $arch."
  if [ -S "$rt/skiff/skiffd.sock" ] && [ ! -e "$d/current" ]; then
    stop "This server runs Skiff from its own install. The Skiff app uses it as it is, and install.sh does not replace it."
  fi
  mkdir -p "$d"
  in=$d/.incoming-$want.tar.gz
  url=https://github.com/solutionscay/skiff/releases/download/v$release/$name
  say "Downloading Skiff $release for $shown $arch…"
  fetch "$url" "$in" || { rm -f "$in"; stop "Could not download Skiff $release. Check that this server can reach github.com."; }
  [ "$(sha "$in")" = "$want" ] || { rm -f "$in"; stop "The download does not match Skiff $release. Nothing changed."; }
  checked="its SHA-256 only: minisign is not installed here"
  if command -v minisign >/dev/null 2>&1 && [ -n "$pubkey" ]; then
    t=$(mktemp -d)
    printf 'untrusted comment: skiff\n%s\n' "$pubkey" > "$t/pub"
    if ! fetch "$url.minisig" "$t/sig" || ! minisign -V -q -p "$t/pub" -m "$in" -x "$t/sig" </dev/null || ! grep -qx "trusted comment: file:$name" "$t/sig"; then
      rm -rf "$t" "$in"; stop "The download of Skiff $release is not signed by Skiff. Nothing changed."
    fi
    rm -rf "$t"
    checked="its SHA-256 and its signature"
  fi
  lock
  out=$(by=install.sh; agreed=$yes; { update "$release" "$want" skiffd.sock "$agreed"; start skiffd.sock; } </dev/null)
  from=$(val "$out" from)
  case $(val "$out" skiff-update) in
    newer) stop "This server runs Skiff $from, newer than Skiff $release. Skiff never moves down. Nothing changed." ;;
    no-handover)
      say "Skiff $from on this server cannot hand its sessions to Skiff $release, so the update ends them:"
      live_list "$rt/skiff/skiffd.sock"
      stop "Nothing changed. Save their work, then run it again with --yes." ;;
    same) say "Skiff $release is installed already. Nothing changed." ;;
    done) ;;
    lost) stop "skiffd started again on Skiff $release instead of reloading. Its sessions ended." ;;
    *) stop "The install did not finish. Skiff ${from:-on this server} is as it was. Run it again." ;;
  esac
  me=$(id -un)
  [ "$(val "$out" skiff-start)" = running ] || stop "Skiff $release is installed, but its sessions service stops right after it starts."
  say ""
  if [ -n "$from" ] && [ "$from" != "$release" ]; then say "Skiff $release runs on this server, as $me. It was $from; its sessions kept running."
  else say "Skiff $release runs on this server, as $me."; fi
  case $(val "$out" how)/$(val "$out" linger) in
    systemd/yes) say "It keeps running after logout and after a restart." ;;
    systemd/*) say "Skiff on this server stops when you log out of every session here. Ask its admin to turn on lingering for $me." ;;
    launchd/*) say "It keeps running after logout. After a restart, it starts when you log in here." ;;
    *) say "It runs until the server restarts. After that, Skiff starts it again when you connect." ;;
  esac
  [ "$(val "$out" host)" = running ] || say "Its projects service (skiff-host) does not stay running. Sessions work; projects, files and Git do not show in the app."
  say "Skiff checked the download: $checked."
  host=$(hostname -f 2>/dev/null || hostname)
  ip=$( (hostname -I 2>/dev/null || ip -o -4 addr show scope global 2>/dev/null | awk '{ sub("/.*", "", $4); print $4 }') | tr ' ' '\n' | grep -v -e '^$' -e '^127\.' -e '^::1$' -e '^fe80' | head -n 1)
  say ""
  say "To use it from your computer: in the Skiff app, open"
  say "Settings › Hosts › Add SSH Host…, then pick or type this server:"
  say "  $me@$host${ip:+   (or $ip)}"
}

case $mode in
  "" | --*) mode=headless; headless "$@" ;;
  probe) probe "${2:-}" "${3:-}" ;;
  install-key) install_key "${2:?}" ;;
  fix-key) fix_key "${2:?}" ;;
  install) mkdir -p "$d"; lock; install "$2" "$3"; start "$4" ;;
  update) mkdir -p "$d"; lock; update "$2" "$3" "$4" "${5:-no}"; start "$4" ;;
  start) start "$2" ;;
  linger) linger; echo "skiff-linger=done" ;;
  sessions) sessions "$2" ;;
  uninstall) uninstall ;;
  *) echo "skiff-$mode=unknown" ;;
esac
