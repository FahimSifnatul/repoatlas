"""Java (and the base for Kotlin): imports are fully-qualified class names, matched to package + file name."""
from __future__ import annotations

import re

from .base import Language, lead_comment, line_of


class JvmIndex:
    """Fully-qualified class name -> file, across Java and Kotlin, built once per project."""

    def __init__(self, project):
        self.fq = {f"{i['pkg']}.{p.rsplit('/', 1)[-1].rsplit('.', 1)[0]}": p
                   for p, i in project.files.items() if i["lang"] in ("Java", "Kotlin") and i["pkg"]}
        self.own = {".".join(k.split(".")[:2]) for k in self.fq}   # e.g. {"com.acme"}

    def resolve(self, path, imports):
        uses, ext = set(), set()
        for imp in imports:
            parts = imp.split(".")
            hit = next((self.fq[".".join(parts[:k])] for k in range(len(parts), 1, -1) if ".".join(parts[:k]) in self.fq), None)
            if hit:
                if hit != path:
                    uses.add(hit)
            elif ".".join(parts[:2]) not in self.own and not imp.startswith(("java.", "kotlin.", "android.")):
                ext.add((".".join(x for x in parts[:3] if not x[:1].isupper()), imp))   # jakarta.persistence, not .Entity
        return uses, ext


class Java(Language):
    name = "Java"
    extensions = (".java",)
    color = "#b07219"
    implicit = "folder"                          # classes in the same package need no import
    type_keywords = "class|interface|enum|record"
    entry_re = r"static\s+void\s+main\s*\(|@SpringBootApplication"
    function_re = None                           # Java has no top-level functions

    def parse(self, f):
        t = f["text"]
        m = re.search(r"^\s*package\s+([\w.]+)", t, re.M)
        f["pkg"] = m.group(1) if m else ""
        f["imports"] = [i.rstrip(".") for i in re.findall(r"^\s*import\s+(?:static\s+)?([\w.]+)", t, re.M)]
        lines = t.splitlines()
        pat = (r"^\s*(?:(?:public|private|protected|internal|abstract|final|open|data|sealed|static)\s+)*"
               rf"({self.type_keywords})\s+(\w+)")
        for m in re.finditer(pat, t, re.M):
            ln = line_of(t, m.start())
            f["symbols"].append(dict(k="class", n=m.group(2), sig=m.group(1), doc=lead_comment(lines, ln - 1), line=ln))
        if self.function_re:
            for m in re.finditer(self.function_re, t, re.M):
                ln = line_of(t, m.start())
                f["symbols"].append(dict(k="function", n=m.group(1), sig="", doc=lead_comment(lines, ln - 1), line=ln))

    def prepare(self, project):
        self.index = project.shared("jvm", JvmIndex)

    def resolve(self, project, f):
        return self.index.resolve(f["path"], f["imports"])
