#!/bin/sh
# repoatlas installer.
#
# Puts ONE self-contained file called `repoatlas` on your PATH. No sudo, nothing else is touched,
# and once it is installed you can delete everything you downloaded.
#
#   curl -fsSL https://raw.githubusercontent.com/FahimSifnatul/repoatlas/master/install.sh | sh
#   ./install.sh                  from a clone of the repo (builds from the clone)
#   ./install.sh --uninstall      remove it again
#
# Needs: Python 3.8+ (repoatlas is a Python program) and curl or wget (not needed from a clone).
#
# Optional environment variables:
#   REPOATLAS_INSTALL_DIR      where to put the file             (default: ~/.local/bin)
#   REPOATLAS_VERSION          release tag such as v1.0.0        (default: the latest release)
#   REPOATLAS_NO_MODIFY_PATH=1 do not edit any shell startup file
#   REPOATLAS_REPO, REPOATLAS_BASE_URL, REPOATLAS_REF   for forks, mirrors and testing

set -eu

NAME=repoatlas
REPO="${REPOATLAS_REPO:-FahimSifnatul/repoatlas}"
BASE="${REPOATLAS_BASE_URL:-https://github.com}"
VERSION="${REPOATLAS_VERSION:-latest}"
DIR="${REPOATLAS_INSTALL_DIR:-$HOME/.local/bin}"
REF="${REPOATLAS_REF:-master}"
MARK="# added by the repoatlas installer"
TMP=""

say()  { printf '%s\n' "$*"; }
warn() { printf 'repoatlas installer: %s\n' "$*" >&2; }
die()  { warn "$*"; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
cleanup() { [ -n "$TMP" ] && rm -rf "$TMP"; return 0; }
trap cleanup EXIT
trap 'exit 130' INT TERM

usage() {
  cat <<'EOF'
repoatlas installer: puts one self-contained file called `repoatlas` on your PATH.

  install.sh                    install (or update)
  install.sh --uninstall        remove it again
  install.sh --no-modify-path   do not edit shell startup files

Environment: REPOATLAS_INSTALL_DIR (default ~/.local/bin), REPOATLAS_VERSION (e.g. v1.0.0),
REPOATLAS_NO_MODIFY_PATH=1. Needs Python 3.8+ and, unless run from a clone, curl or wget.
EOF
}

# ---------------------------------------------------------------- arguments
ACTION=install
for arg in "$@"; do
  case "$arg" in
    --uninstall) ACTION=uninstall ;;
    --no-modify-path) REPOATLAS_NO_MODIFY_PATH=1 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $arg (try --help)" ;;
  esac
done

# ---------------------------------------------------------------- uninstall
if [ "$ACTION" = uninstall ]; then
  target="$DIR/$NAME"
  [ -f "$target" ] || target=$(command -v "$NAME" 2>/dev/null || true)
  if [ -z "$target" ] || [ ! -f "$target" ]; then die "repoatlas is not installed (looked in $DIR and on your PATH)."; fi
  # only delete a file that really is repoatlas, never some other program with the same name
  "$target" --version 2>/dev/null | grep -q "^repoatlas " || die "$target does not look like repoatlas, leaving it alone."
  if [ -w "$(dirname "$target")" ]; then rm -f "$target"; else sudo rm -f "$target"; fi
  say "Removed $target"
  say "(A PATH line the installer added to your shell startup file is harmless; delete it if you like.)"
  exit 0
fi

# ---------------------------------------------------------------- requirements
have python3 || die "Python 3 is required because repoatlas is a Python program.
  macOS:           xcode-select --install     (or: brew install python)
  Debian/Ubuntu:   sudo apt install python3
  Fedora:          sudo dnf install python3"
python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)' 2>/dev/null \
  || die "Python 3.8 or newer is required, found: $(python3 -V 2>&1)"

fetch() {  # fetch URL DEST
  if have curl; then curl -fsSL "$1" -o "$2"; else wget -q -O "$2" "$1"; fi
}

TMP=$(mktemp -d "${TMPDIR:-/tmp}/repoatlas-install.XXXXXX")

# ---------------------------------------------------------------- get the file
build_from() {  # build_from SOURCE_DIR
  say "Building repoatlas from source..."
  ( cd "$1" && python3 scripts/build_binary.py >/dev/null ) || die "the build failed."
  cp "$1/dist/$NAME" "$TMP/$NAME"
}

verify_checksum() {
  want=$(cut -d' ' -f1 "$TMP/$NAME.sha256")
  if have sha256sum; then got=$(sha256sum "$TMP/$NAME" | cut -d' ' -f1)
  elif have shasum; then got=$(shasum -a 256 "$TMP/$NAME" | cut -d' ' -f1)
  else return 0
  fi
  [ "$got" = "$want" ] || die "the download is corrupted (checksum mismatch). Nothing was installed."
}

SELF_DIR=""
if [ -f "$0" ]; then SELF_DIR=$(cd "$(dirname "$0")" 2>/dev/null && pwd) || SELF_DIR=""; fi

if [ -n "$SELF_DIR" ] && [ -f "$SELF_DIR/scripts/build_binary.py" ] && [ -d "$SELF_DIR/repoatlas" ]; then
  FROM_CLONE=1
  build_from "$SELF_DIR"
else
  FROM_CLONE=0
  have curl || have wget || die "curl or wget is needed to download repoatlas."
  if [ "$VERSION" = latest ]; then REL="$BASE/$REPO/releases/latest/download"; else REL="$BASE/$REPO/releases/download/$VERSION"; fi
  say "Downloading repoatlas..."
  if fetch "$REL/$NAME" "$TMP/$NAME" 2>/dev/null; then
    if fetch "$REL/$NAME.sha256" "$TMP/$NAME.sha256" 2>/dev/null; then verify_checksum; fi
  else
    # no release published yet: build the current source instead
    say "No release found, building from source..."
    fetch "$BASE/$REPO/archive/refs/heads/$REF.tar.gz" "$TMP/src.tar.gz" \
      || die "could not download from $BASE/$REPO (is the repository public and the name right?)"
    tar -xzf "$TMP/src.tar.gz" -C "$TMP"
    SRC=""
    for d in "$TMP"/*/; do [ -f "${d}scripts/build_binary.py" ] && SRC=${d%/}; done
    [ -n "$SRC" ] || die "the downloaded source is not what I expected."
    build_from "$SRC"
  fi
fi

# ---------------------------------------------------------------- install
chmod 755 "$TMP/$NAME"
"$TMP/$NAME" --version >/dev/null 2>&1 || die "the downloaded file does not run, nothing was installed."

if mkdir -p "$DIR" 2>/dev/null && [ -w "$DIR" ]; then
  cp "$TMP/$NAME" "$DIR/$NAME.new" && chmod 755 "$DIR/$NAME.new" && mv -f "$DIR/$NAME.new" "$DIR/$NAME"
else
  have sudo || die "$DIR is not writable and sudo is not available. Set REPOATLAS_INSTALL_DIR to a folder you own."
  say "$DIR needs administrator rights."
  sudo mkdir -p "$DIR" && sudo cp "$TMP/$NAME" "$DIR/$NAME" && sudo chmod 755 "$DIR/$NAME"
fi
INSTALLED=$("$DIR/$NAME" --version)
say "Installed $INSTALLED at $DIR/$NAME"

# ---------------------------------------------------------------- put it on the PATH
on_path() { case ":$PATH:" in *":$DIR:"*) return 0 ;; esac; return 1; }

add_to_path() {
  case "$DIR" in "$HOME"/*) shown="\$HOME${DIR#"$HOME"}" ;; *) shown=$DIR ;; esac
  shell=$(basename "${SHELL:-sh}")
  case "$shell" in
    zsh)  rc="${ZDOTDIR:-$HOME}/.zshrc" ;;
    bash) if [ "$(uname -s)" = Darwin ]; then          # Terminal.app starts login shells: .bashrc is not read
            if [ -f "$HOME/.bash_profile" ] || [ ! -f "$HOME/.profile" ]; then rc="$HOME/.bash_profile"; else rc="$HOME/.profile"; fi
          else rc="$HOME/.bashrc"; fi ;;
    fish) rc="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/repoatlas.fish" ;;
    *)    rc="$HOME/.profile" ;;
  esac
  if [ "$shell" = fish ]; then
    line="fish_add_path $shown  $MARK"
  else
    line="export PATH=\"$shown:\$PATH\"  $MARK"
  fi
  if [ -f "$rc" ] && grep -qF "$line" "$rc"; then return 0; fi
  mkdir -p "$(dirname "$rc")"
  printf '\n%s\n' "$line" >> "$rc"
  say "Added $shown to your PATH in $rc"
}

if on_path; then
  :
elif [ "${REPOATLAS_NO_MODIFY_PATH:-0}" = 1 ]; then
  say "$DIR is not on your PATH. Add it yourself:  export PATH=\"$DIR:\$PATH\""
else
  add_to_path
  NEED_NEW_SHELL=1
fi

# another repoatlas earlier on the PATH (pipx, Homebrew...) would hide this one
other=$(command -v "$NAME" 2>/dev/null || true)
if [ -n "$other" ] && [ "$other" != "$DIR/$NAME" ]; then
  warn "note: $other comes first on your PATH and will run instead of the one just installed."
fi

# ---------------------------------------------------------------- done
say ""
if [ "${NEED_NEW_SHELL:-0}" = 1 ]; then
  say "Open a new terminal window, then run:   repoatlas --help"
  say "(or in this window right now:   export PATH=\"$DIR:\$PATH\")"
else
  say "Ready. Try:   repoatlas --help"
fi
if [ "$FROM_CLONE" = 1 ]; then
  say ""
  say "repoatlas is one self-contained file, so you can delete the folder you installed from."
fi
