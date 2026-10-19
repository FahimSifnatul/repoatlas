"""C: local `#include "x.h"` lines are followed (next to the file, then include/, src/, the repo root)."""
from __future__ import annotations

import posixpath
import re

from .base import Language, lead_comment, line_of


class C(Language):
    name = "C"
    extensions = (".c", ".h")
    color = "#555555"
    entry_by_name = True

    def parse(self, f):
        t = f["text"]
        f["imports"] = re.findall(r'^\s*#\s*include\s+"([^"]+)"', t, re.M)
        f["entry"] = bool(re.search(r"\bint\s+main\s*\(", t))
        lines = t.splitlines()
        for m in re.finditer(r"^\s*(?:typedef\s+)?(class|struct|enum)\s+(\w+)\s*(?:[:{]|$)", t, re.M):
            ln = line_of(t, m.start())
            f["symbols"].append(dict(k="class", n=m.group(2), sig=m.group(1), doc=lead_comment(lines, ln - 1), line=ln))

    def resolve(self, project, f):
        uses = set()
        for inc in f["imports"]:
            for base in (posixpath.dirname(f["path"]), "include", "src", ""):
                c = posixpath.normpath(posixpath.join(base, inc))
                if c in project.paths and c != f["path"]:
                    uses.add(c)
                    break
        return uses, set()
