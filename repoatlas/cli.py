"""Command line: `repoatlas <repo>`."""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
import webbrowser
from pathlib import Path

from . import __version__
from .core import analyze, render

EPILOG = """examples:
  repoatlas                                  map the current folder
  repoatlas ~/code/my-app                    map a local repository
  repoatlas https://github.com/pallets/flask map a repository straight from GitHub
  repoatlas . -o map.html --no-open          choose the output file, do not open the browser
"""


def _is_url(s: str) -> bool:
    return bool(re.match(r"^(https?://|git@|ssh://)", s))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(
        prog="repoatlas",
        description="Turn a code repository into an interactive, layer-by-layer architecture map (one HTML file).",
        epilog=EPILOG, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo", nargs="?", default=".", help="local folder or git URL (default: current folder)")
    ap.add_argument("-o", "--out", default="repoatlas.html", help="output file (default: repoatlas.html)")
    ap.add_argument("--no-open", action="store_true", help="do not open the result in a browser")
    ap.add_argument("-V", "--version", action="version", version=f"%(prog)s {__version__}")
    a = ap.parse_args(argv)

    out = Path(a.out).expanduser().resolve()
    if _is_url(a.repo):
        name = re.sub(r"(\.git)?/?$", "", a.repo).rsplit("/", 1)[-1]
        with tempfile.TemporaryDirectory(prefix="repoatlas-") as tmp:      # the clone is removed afterwards
            print("Cloning…", file=sys.stderr)
            try:
                subprocess.run(["git", "clone", "--depth", "1", "--quiet", a.repo, tmp], check=True)
            except FileNotFoundError:
                return _fail("git is not installed, so repoatlas cannot clone a URL. Clone it yourself and pass the folder.")
            except subprocess.CalledProcessError:
                return _fail(f"could not clone {a.repo}")
            data = analyze(Path(tmp), name)
    else:
        root = Path(a.repo).expanduser().resolve()
        if not root.is_dir():
            return _fail(f"'{a.repo}' is not a folder")
        data = analyze(root, root.name)

    out.write_text(render(data, data["repo"]["name"]), encoding="utf-8")
    r = data["repo"]
    print(f"Scanned {r['files']} files ({r['loc']:,} lines): {len(data['layers'])} layers, {r['parts']} parts.\nWrote {out}")
    if not a.no_open:
        webbrowser.open(out.as_uri())
    return 0


def _fail(msg: str) -> int:
    print(f"repoatlas: {msg}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
