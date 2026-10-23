# Skiff: add this computer's public key to ~/.ssh/authorized_keys, once.
# $1: the key line, checked by Skiff against a strict pattern. It adds the
# line only when that exact line is not there, and changes nothing else
# except the folder and file modes of what the account owns.
key=$1
umask 077
me=$(id -u)
owner() { ls -nd "$1" 2>/dev/null | awk '{print $3}'; }
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
[ "$(owner "$HOME/.ssh")" = "$me" ] && chmod 700 "$HOME/.ssh" && dir=yes || dir=no
[ "$(owner "$ak")" = "$me" ] && chmod 600 "$ak" && file=yes || file=no
echo "added=$added"
echo "ssh_dir_own=$dir"
echo "file_own=$file"
echo "skiff-install=done"
