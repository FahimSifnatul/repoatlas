"""Go: imports are packages (folders), so each import is linked to the most representative file in that folder."""
from __future__ import annotations

import posixpath
import re
from collections import defaultdict

from .base import Language, lead_comment, line_of


class Go(Language):
    name = "Go"
    extensions = (".go",)
    color = "#00ADD8"
    manifests = ("go.mod",)
    entry_by_name = True
    implicit = "folder"                   # files of one package share one folder and need no import

    def parse(self, f):
        t = f["text"]
        for blk in re.findall(r"^import\s*\((.*?)^\)", t, re.M | re.S):   # up to the closing line: comments may hold ")"
            f["imports"] += re.findall(r'"([^"]+)"', blk)
        f["imports"] += re.findall(r'^import\s+(?:\w+\s+)?"([^"]+)"', t, re.M)
        f["entry"] = bool(re.search(r"^package\s+main\b", t, re.M) and re.search(r"^func\s+main\s*\(", t, re.M))
        lines = t.splitlines()
        for m in re.finditer(r"^func\s+(?:\(\s*\w+\s+\*?([\w\[\]]+)\s*\)\s*)?(\w+)\s*\(", t, re.M):
            ln = line_of(t, m.start())
            f["symbols"].append(dict(k="function", n=m.group(2), sig=f"on {m.group(1)}" if m.group(1) else "",
                                     doc=lead_comment(lines, ln - 1), line=ln))
        for m in re.finditer(r"^type\s+(\w+)\s+(struct|interface)", t, re.M):
            ln = line_of(t, m.start())
            f["symbols"].append(dict(k="class" if m.group(2) == "struct" else "type", n=m.group(1), sig=m.group(2),
                                     doc=lead_comment(lines, ln - 1), line=ln))
        f["symbols"].sort(key=lambda s: (not s["n"][:1].isupper(), s["line"]))   # exported names first

    def prepare(self, project):
        self.modules = {}                        # module path from go.mod -> folder
        for rel in project.man.get("go.mod", []):
            m = re.search(r"^module\s+(\S+)", project.read(rel), re.M)
            if m:
                self.modules[m.group(1)] = posixpath.dirname(rel)
        self.packages = defaultdict(list)        # folder -> its non-test .go files
        for p, i in project.files.items():
            if i["lang"] == self.name and not p.endswith("_test.go"):
                self.packages[posixpath.dirname(p)].append(p)

    def resolve(self, project, f):
        uses, ext = set(), set()
        for imp in f["imports"]:
            hit = None
            for mod, d in self.modules.items():
                if imp == mod or imp.startswith(mod + "/"):
                    target = posixpath.normpath(posixpath.join(d, imp[len(mod) + 1:])) if imp != mod else d
                    target = "" if target == "." else target
                    fs = [p for p in self.packages.get(target, []) if p != f["path"]]
                    if fs:
                        base = posixpath.basename(target)
                        hit = (next((p for p in fs if posixpath.basename(p) == base + ".go"), None)
                               or max(fs, key=lambda p: project.files[p]["loc"]))
                    break
            if hit:
                uses.add(hit)
            elif "." in imp.split("/")[0]:       # github.com/x/y is outside; "fmt" is the standard library
                ext.add(("/".join(imp.split("/")[:3]), imp))
        return uses, ext
