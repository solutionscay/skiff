# Skiff's managed install on a Linux server, in the account's home only. No
# sudo, no system folder. $1: install, update, start or linger. install: $2
# the version, $3 the SHA-256 of the bundle Skiff uploaded to
# ~/.local/share/skiff/server/.incoming-<sha256>.tar.gz, $4 the skiffd socket
# name. update: the same, and $5 yes when the person agreed that sessions
# end if skiffd cannot hand them over. start: $2 the socket name. Each step
# prints key=value lines and ends with skiff-<mode>=<outcome>. It never stops
# or replaces a skiffd it did not start: Skiff only runs it after the check
# found no other one.
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
    rm -rf "${d:?}/$v"
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
    ln -sfn "$old" "$d/current"; start "$3" >/dev/null; echo "skiff-update=failed"; return
  fi
  # The old skiffd runs on, untouched: it cannot hand its sessions over.
  live=$("$cli" --socket "$sock" ls --json 2>/dev/null | tr -d '\n')
  if [ "$agreed" != yes ] && ! printf '%s' "$live" | grep -Eq '"state": *"(working|idle|waiting)"'; then agreed=yes; fi
  if [ "$agreed" != yes ]; then
    ln -sfn "$old" "$d/current"
    echo "sessions=$live"; echo "skiff-update=no-handover"; return
  fi
  "$cli" --socket "$sock" daemon stop >/dev/null 2>&1
  if has_systemd; then systemctl --user restart skiffd
  elif [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then kill "$pid"; sleep 1; fi
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
sessions() { "$d/current/skiff" --socket "$1" ls 2>/dev/null </dev/null | sed 's/^/  /'; }

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

uninstall() {
  sock=$rt/skiff/skiffd.sock
  [ -e "$d/current" ] || stop "Skiff is not installed in ~/.local/share/skiff/server. Nothing changed."
  n=$(live_count "$sock")
  if [ "${n:-0}" -gt 0 ]; then
    say "Uninstall ends these sessions in Skiff on this server:"
    sessions "$sock"
    agree "Save their work first." || stop "Nothing changed. Run it again with --yes to end them."
  fi
  # Only the skiffd this install started: its unit, or its pid file.
  if has_systemd; then
    systemctl --user disable --now skiffd skiff-host >/dev/null 2>&1
    rm -f "$HOME/.config/systemd/user/skiffd.service" "$HOME/.config/systemd/user/skiff-host.service"
    systemctl --user daemon-reload
  fi
  pid=$(cat "$d/skiffd.pid" 2>/dev/null)
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  l=$HOME/.local/bin/skiff
  [ "$(readlink "$l" 2>/dev/null)" = "$d/current/skiff" ] && rm -f "$l"
  rm -rf "$d"
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
  [ "$un" = yes ] && { uninstall; return; }
  os=$(uname -s) arch=$(uname -m)
  case $arch in amd64) arch=x86_64 ;; arm64) arch=aarch64 ;; esac
  case $os/$arch in
    Linux/x86_64 | Linux/aarch64) ;;
    Darwin/x86_64 | Darwin/aarch64) stop "Skiff installs itself on Linux servers only, for now. Install the Skiff app on this Mac." ;;
    *) stop "This server runs $os on $arch. Skiff runs on Linux and macOS, on x86_64 and ARM64." ;;
  esac
  [ -n "$release" ] || stop "This install.sh names no release. Download it from a Skiff release."
  name=skiff-server-$release-linux-$arch.tar.gz
  want=$(printf '%s\n' "$sums" | awk -v n="$name" '$2 == n { print $1 }')
  [ -n "$want" ] || stop "Skiff $release has no bundle for Linux $arch."
  if [ -S "$rt/skiff/skiffd.sock" ] && [ ! -e "$d/current" ]; then
    stop "This server runs Skiff from its own install. The Skiff app uses it as it is, and install.sh does not replace it."
  fi
  mkdir -p "$d"
  in=$d/.incoming-$want.tar.gz
  url=https://github.com/solutionscay/skiff/releases/download/v$release/$name
  say "Downloading Skiff $release for Linux $arch…"
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
      sessions "$rt/skiff/skiffd.sock"
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
    *) say "It runs until the server restarts. After that, Skiff starts it again when you connect." ;;
  esac
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
  install) mkdir -p "$d"; lock; install "$2" "$3"; start "$4" ;;
  update) mkdir -p "$d"; lock; update "$2" "$3" "$4" "${5:-no}"; start "$4" ;;
  start) start "$2" ;;
  linger) linger; echo "skiff-linger=done" ;;
  *) echo "skiff-$mode=unknown" ;;
esac
