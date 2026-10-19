#!/usr/bin/env python3
"""Build dist/repoatlas: ONE executable file, no install step.

It is a Python "zipapp": the whole package (viewer and all language files included) zipped behind a
`#!/usr/bin/env python3` line. Copy it anywhere on your PATH and run `repoatlas <repo>`.
It needs Python 3.8+ on the machine, nothing else.

    python scripts/build_binary.py
"""
import shutil
import stat
import sys
import tempfile
import zipapp
from pathlib import Path

root = Path(__file__).resolve().parent.parent
out = root / "dist" / "repoatlas"

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    shutil.copytree(root / "repoatlas", tmp / "repoatlas", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    (tmp / "__main__.py").write_text("from repoatlas.cli import main\n\nraise SystemExit(main())\n")
    out.parent.mkdir(exist_ok=True)
    zipapp.create_archive(tmp, out, interpreter="/usr/bin/env python3", compressed=True)

out.chmod(out.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
print(f"built {out.relative_to(root)} ({out.stat().st_size // 1024} KB)", file=sys.stderr)
