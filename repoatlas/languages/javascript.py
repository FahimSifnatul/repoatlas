"""JavaScript. TypeScript, Vue and Svelte reuse everything here (same import rules)."""
from __future__ import annotations

import json
import posixpath
import re

from .base import Language, lead_comment

IMPORTS = [
    re.compile(r"""\bimport\s+(?:[^'"();]*?\sfrom\s+)?['"]([^'"\n]+)['"]"""),
    re.compile(r"""\bexport\s+(?:type\s+)?(?:\*(?:\s+as\s+\w+)?|\{[^}]*\})\s*from\s*['"]([^'"\n]+)['"]"""),
    re.compile(r"""\brequire\(\s*['"]([^'"\n]+)['"]\s*\)"""),
    re.compile(r"""\bimport\(\s*['"]([^'"\n]+)['"]\s*\)"""),
]
SYMBOLS = [
    (re.compile(r"^(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)", re.M), "class"),
    (re.compile(r"^(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)\s*\(", re.M), "function"),
    (re.compile(r"^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=\n]+)?=\s*(?:async\s*)?"
                r"(?:function\b|\([^)\n]*\)\s*(?::[^=\n]+)?=>|[A-Za-z_$][\w$]*\s*=>)", re.M), "function"),
    (re.compile(r"^(?:export\s+)?(?:interface|type|enum)\s+([A-Za-z_$][\w$]*)", re.M), "type"),
]
EXTENSIONS = ("", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".vue", ".svelte")
NODE_BUILTIN = set(
    "fs path http https os util crypto events stream url child_process zlib assert buffer net dns readline "
    "cluster tls querystring timers process module worker_threads perf_hooks".split()
)


class JsIndex:
    """Import rules for the whole JS family, built once per project."""

    def __init__(self, project):
        self.paths = project.paths
        self.dirnames = {x.split("/")[-1] for x in project.dirs if x.count("/") <= 1}
        self.pkgs = {}                           # workspace package name -> folder
        for rel in project.man.get("package.json", []):
            try:
                j = json.loads(project.read(rel))
            except ValueError:
                continue
            if not isinstance(j, dict):
                continue
            d = posixpath.dirname(rel)
            if j.get("name"):
                self.pkgs[j["name"]] = d
            cands = [j.get("main")] + (list(j["bin"].values()) if isinstance(j.get("bin"), dict) else [j.get("bin")])
            for s in (j.get("scripts") or {}).values():
                m = re.search(r"(?:node|ts-node|tsx|nodemon|bun|deno run)\s+([\w./-]+\.(?:js|ts|mjs|cjs))", str(s))
                cands.append(m.group(1) if m else None)
            for c in cands:
                if isinstance(c, str):
                    h = self.file(posixpath.normpath(posixpath.join(d, c)))
                    if h:
                        project.declared.append(h)

    def file(self, base):
        """'./a' -> a.ts / a.js / a/index.js ...; also './a.js' written for an a.ts source."""
        base = base.strip("/")
        cands = [base]
        m = re.match(r"(.*)\.(?:m|c)?jsx?$", base)
        if m:
            cands += [m.group(1) + e for e in (".ts", ".tsx")]
        for c in cands:
            for e in EXTENSIONS:
                if c + e in self.paths:
                    return c + e
            for e in EXTENSIONS[1:]:
                if f"{c}/index{e}" in self.paths:
                    return f"{c}/index{e}"
        return None

    def resolve(self, path, specs):
        uses, ext = set(), set()
        for s in specs:
            if s.startswith("."):
                h = self.file(posixpath.normpath(posixpath.join(posixpath.dirname(path), s)))
                if h:
                    uses.add(h)
                continue
            hit = None
            if s.startswith(("@/", "~/", "#/")):  # path alias: look in every enclosing folder's src/ (monorepo friendly)
                anc, d = [], posixpath.dirname(path)
                while True:
                    anc.append(d)
                    if not d:
                        break
                    d = posixpath.dirname(d)
                hit = next((h for a in anc for r in ("src", "app", "") for h in [self.file(posixpath.join(a, r, s[2:]))] if h), None)
                if not hit:
                    continue  # an alias we cannot resolve is not an outside library
            else:
                for name, d in self.pkgs.items():
                    if s == name or s.startswith(name + "/"):
                        rest = s[len(name) + 1:]
                        hit = next((h for c in ([f"{d}/{rest}", f"{d}/src/{rest}"] if rest else [f"{d}/src/index", f"{d}/index"])
                                    for h in [self.file(c)] if h), None)
                        break
                if not hit and not s.startswith("@") and s.split("/")[0] in self.dirnames:
                    hit = next((h for r in ("src", "") for h in [self.file(f"{r}/{s}")] if h), None)
            if hit:
                uses.add(hit)
            else:
                if s.startswith("node:") or s in NODE_BUILTIN:
                    continue
                parts = s.split("/")
                ext.add(("/".join(parts[:2]) if s.startswith("@") else parts[0], s))
        return uses, ext


class JavaScript(Language):
    name = "JavaScript"
    extensions = (".js", ".jsx", ".mjs", ".cjs")
    interpreters = ("node", "nodejs", "bun", "deno")
    color = "#d9b611"
    manifests = ("package.json",)
    entry_by_name = True
    implicit = "none"                     # every use is an import

    def parse(self, f):
        f["imports"] = [s for rx in IMPORTS for s in rx.findall(f["text"])]
        lines, seen = f["text"].splitlines(), set()
        for rx, kind in SYMBOLS:
            for m in rx.finditer(f["text"]):
                name = m.group(m.lastindex)
                ln = f["text"].count("\n", 0, m.start()) + 1
                if (name, ln) in seen:
                    continue
                seen.add((name, ln))
                is_component = kind == "function" and f["path"].endswith((".jsx", ".tsx")) and name[:1].isupper()
                f["symbols"].append(dict(k="component" if is_component else kind, n=name, sig="",
                                         doc=lead_comment(lines, ln - 1), line=ln))
        f["symbols"].sort(key=lambda s: s["line"])

    def prepare(self, project):
        self.index = project.shared("js", JsIndex)

    def resolve(self, project, f):
        return self.index.resolve(f["path"], f["imports"])

    def layer_hint(self, f):
        return "interface" if f["path"].endswith((".jsx", ".tsx")) else super().layer_hint(f)
