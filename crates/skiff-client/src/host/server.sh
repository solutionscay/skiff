# Skiff's managed install on a Linux or macOS server, in the account's home only. No
# sudo, no system folder. $1: install, start or linger. install: $2 the
# version, $3 the SHA-256 of the bundle Skiff uploaded to
# ~/.local/share/skiff/server/.incoming.tar.gz, $4 the skiffd socket name.
# start: $2 the socket name. Each step prints key=value lines and ends with
# skiff-<mode>=<outcome>. It never stops or replaces a skiffd it did not
# start: Skiff only runs it after the check found no other one.
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

# macOS: a LaunchAgent. launchd starts it at login and after a crash. A
# second skiffd on a live socket exits with an error, so it stays down.
label=com.solutionscay.skiff.skiffd
xml() { printf '%s' "$1" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g'; }
plist() {
  cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$(xml "$d/current/skiffd")</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>Crashed</key>
    <true/>
  </dict>
  <key>ProcessType</key>
  <string>Interactive</string>
</dict>
</plist>
EOF
}
# The launchd domain that has the agent loaded, if any.
loaded() {
  for x in "gui/$uid" "user/$uid"; do
    if launchctl print "$x/$label" >/dev/null 2>&1; then echo "$x"; return; fi
  done
}
agent() {
  a=$HOME/Library/LaunchAgents
  mkdir -p "$a"
  # A changed file waits for the next load: a reload ends every session.
  plist > "$a/.$label.plist" && mv "$a/.$label.plist" "$a/$label.plist"
  on=$(loaded)
  if [ -n "$on" ]; then
    # Starts it only if it does not run.
    launchctl kickstart "$on/$label" >/dev/null 2>&1
  else
    # Homebrew's rule: the GUI domain when this account owns the screen,
    # else the user domain, which an ssh login reaches and which keeps
    # running after logout.
    on=user/$uid
    [ "$(ls -ln /dev/console 2>/dev/null | awk '{print $3}')" = "$uid" ] && on=gui/$uid
    launchctl bootstrap "$on" "$a/$label.plist" >/dev/null 2>&1
  fi
  echo how=launchd
  echo "domain=${on%%/*}"
}

# Starts the managed skiffd if it does not run, and waits for its socket.
start() {
  sock=$rt/skiff/$1
  [ -x "$d/current/skiffd" ] || { echo "skiff-start=missing"; return; }
  if [ "$os" = Darwin ]; then
    agent
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

case $mode in
  install) mkdir -p "$d"; install "$2" "$3"; start "$4" ;;
  start) start "$2" ;;
  linger) linger; echo "skiff-linger=done" ;;
  *) echo "skiff-$mode=unknown" ;;
esac
