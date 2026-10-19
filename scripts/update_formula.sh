#!/bin/sh
# Point the Homebrew formula at a published release: fills in the tarball URL and its sha256.
# Run it AFTER you pushed the tag (the tarball has to exist on GitHub):
#
#     scripts/update_formula.sh v1.0.0
#
# then copy packaging/homebrew/Formula/repoatlas.rb into your homebrew-tap repository.
set -eu

tag=${1:?usage: scripts/update_formula.sh vX.Y.Z}
cd "$(dirname "$0")/.."
formula=packaging/homebrew/Formula/repoatlas.rb

home=$(sed -n 's/^  homepage "\(.*\)"/\1/p' "$formula")
placeholder="YOUR_GITHUB_""USER"          # written in two pieces so personalize.sh never rewrites it
case "$home" in
  *"$placeholder"*) echo "Run scripts/personalize.sh <your-github-username> first." >&2; exit 1 ;;
esac
url=${REPOATLAS_TARBALL_URL:-$home/archive/refs/tags/$tag.tar.gz}

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
curl -fsSL "$url" -o "$tmp" || { echo "Could not download $url. Is the tag pushed?" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum "$tmp" | cut -d' ' -f1); else sum=$(shasum -a 256 "$tmp" | cut -d' ' -f1); fi

sed -i.bak -e "s|^  url \".*\"|  url \"$url\"|" -e "s|^  sha256 \".*\"|  sha256 \"$sum\"|" "$formula" && rm -f "$formula.bak"
echo "Updated $formula"
echo "  url    $url"
echo "  sha256 $sum"
