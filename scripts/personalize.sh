#!/bin/sh
# Put your GitHub username into every placeholder (README, installer, Homebrew formula, docs).
# Run it once, before the first push:
#
#     scripts/personalize.sh octocat
set -eu

[ $# -eq 1 ] || { echo "usage: $0 <your-github-username>" >&2; exit 1; }
user=$1
case "$user" in
  ""|*[!A-Za-z0-9-]*|-*) echo "'$user' does not look like a GitHub username" >&2; exit 1 ;;
esac

cd "$(dirname "$0")/.."
placeholder="YOUR_GITHUB_""USER"          # written in two pieces so this script never rewrites itself
files=$(grep -rIl "$placeholder" . --exclude-dir=.git --exclude-dir=dist --exclude-dir=target --exclude-dir=__pycache__ 2>/dev/null || true)
printf '%s\n' "$files" | while IFS= read -r f; do
  [ -n "$f" ] && sed -i.bak "s/$placeholder/$user/g" "$f" && rm -f "$f.bak"
done
count=$(printf '%s\n' "$files" | grep -c . || true)
echo "Replaced the placeholder in $count files. Your repository will live at https://github.com/$user/repoatlas"
