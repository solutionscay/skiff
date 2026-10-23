# Skiff: ~/.ssh/authorized_keys belongs to another account, so sshd ignores
# it. The account owns ~/.ssh, so it can rename the file. $1: this
# computer's key line, checked by Skiff. The old file stays, renamed.
key=$1
umask 077
me=$(id -u)
owner() { ls -nd "$1" 2>/dev/null | awk '{print $3}'; }
if [ "$(owner "$HOME/.ssh")" != "$me" ]; then
  echo "skiff-fix=not-own"
  exit 0
fi
ak=$HOME/.ssh/authorized_keys
if [ -e "$ak" ]; then
  mv -- "$ak" "$ak.skiff-$(date +%Y%m%d%H%M%S)" || exit 1
fi
printf '%s\n' "$key" > "$ak" && chmod 600 "$ak" && chmod 700 "$HOME/.ssh" && echo "skiff-fix=done"
