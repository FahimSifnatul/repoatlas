"""Rust: `mod x;` and `use crate::a::b` are followed to src/a/b.rs or src/a/b/mod.rs."""
from __future__ import annotations

import posixpath
import re

from .base import Language, lead_comment, line_of


class Rust(Language):
    name = "Rust"
    extensions = (".rs",)
    color = "#b7603c"
    manifests = ("Cargo.toml",)
    entry_by_name = True
    implicit = "none"                     # every use is a `mod` or `use`

    def parse(self, f):
        t = f["text"]
        f["mods"] = re.findall(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;", t, re.M)
        f["imports"] = [u.strip(":") for u in re.findall(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([\w:]+)", t, re.M)]
        f["entry"] = bool(re.search(r"^\s*fn\s+main\s*\(", t, re.M))
        lines = t.splitlines()
        for m in re.finditer(r"^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?(struct|enum|trait|fn|type)\s+(\w+)", t, re.M):
            ln = line_of(t, m.start())
            kind = "function" if m.group(1) == "fn" else "class" if m.group(1) in ("struct", "enum", "trait") else "type"
            f["symbols"].append(dict(k=kind, n=m.group(2), sig=m.group(1), doc=lead_comment(lines, ln - 1), line=ln))

    def prepare(self, project):
        self.crates = {}                         # workspace crate name -> folder
        for rel in project.man.get("Cargo.toml", []):
            m = re.search(r'^\s*name\s*=\s*"([^"]+)"', project.read(rel), re.M)
            if m:
                self.crates[m.group(1).replace("-", "_")] = posixpath.dirname(rel)

    def resolve(self, project, f):
        uses, ext = set(), set()
        path = f["path"]
        d, stem = posixpath.dirname(path), posixpath.basename(path)[:-3]
        moddir = d if stem in ("mod", "lib", "main") else posixpath.join(d, stem)
        dparts = d.split("/") if d else []
        src = "/".join(dparts[: len(dparts) - dparts[::-1].index("src")]) if "src" in dparts else ""

        def find(base, rest):
            for k in range(len(rest), 0, -1):
                b = posixpath.join(base, *rest[:k])
                for c in (b + ".rs", b + "/mod.rs"):
                    if c in project.paths and c != path:
                        return c
            return None

        for m in f["mods"]:
            h = find(moddir, [m])
            if h:
                uses.add(h)
        for u in f["imports"]:
            parts = [x for x in u.split("::") if x]
            if not parts or parts[0] in ("std", "core", "alloc"):
                continue
            head, rest = parts[0], parts[1:]
            if head == "crate":
                base = src
            elif head == "self":
                base = moddir
            elif head == "super":
                base = moddir
                while parts and parts[0] == "super":
                    base, parts = posixpath.dirname(base), parts[1:]
                rest = parts
            elif head in f["mods"]:
                base, rest = moddir, parts
            elif head in self.crates:
                base = posixpath.join(self.crates[head], "src")
            else:
                ext.add((head, head))
                continue
            h = find(base, rest) if rest else None
            if h:
                uses.add(h)
        return uses, ext
