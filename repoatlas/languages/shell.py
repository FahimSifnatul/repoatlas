"""Shell scripts: functions are listed; scripts they `source` or run are linked through their file paths."""
from __future__ import annotations

import re

from .base import Language, lead_comment, line_of


class Shell(Language):
    name = "Shell"
    extensions = (".sh", ".bash", ".zsh")
    interpreters = ("sh", "bash", "zsh", "dash", "ash")
    color = "#89e051"
    implicit = "none"             # other scripts are named by path, which the core follows

    def parse(self, f):
        t = f["text"]
        lines = t.splitlines()
        for m in re.finditer(r"^\s*(?:function\s+([\w-]+)|([A-Za-z_][\w-]*)\s*\(\))", t, re.M):   # f() { / function f
            ln = line_of(t, m.start())
            f["symbols"].append(dict(k="function", n=m.group(1) or m.group(2), sig="", doc=lead_comment(lines, ln - 1), line=ln))
