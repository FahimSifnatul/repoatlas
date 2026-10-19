"""Python: symbols and imports come from the real `ast` module, so they are exact."""
from __future__ import annotations

import ast
import posixpath
import re
import sys

from ._python_stdlib import NAMES as _STDLIB
from .base import Language, first_sentence, generic_symbols

STDLIB = set(_STDLIB) | set(getattr(sys, "stdlib_module_names", ()))
HTTP_VERBS = {"get", "post", "put", "delete", "patch", "route", "head", "options", "websocket"}


def _signature(fn) -> str:
    a = fn.args
    names = [x.arg for x in a.posonlyargs + a.args if x.arg not in ("self", "cls")]
    if a.vararg:
        names.append("*" + a.vararg.arg)
    names += [x.arg for x in a.kwonlyargs]
    if a.kwarg:
        names.append("**" + a.kwarg.arg)
    return "(" + ", ".join(names[:5]) + (", …" if len(names) > 5 else "") + ")"


def _route(fn):
    """('GET', '/users') for @app.get('/users') / @router.post(...) / @app.route(..., methods=[...])."""
    for d in fn.decorator_list:
        if (isinstance(d, ast.Call) and isinstance(d.func, ast.Attribute) and d.func.attr.lower() in HTTP_VERBS
                and d.args and isinstance(d.args[0], ast.Constant) and isinstance(d.args[0].value, str)):
            verb = d.func.attr.upper()
            if verb == "ROUTE":
                verb = "ANY"
                for kw in d.keywords:
                    if kw.arg == "methods" and isinstance(kw.value, (ast.List, ast.Tuple)):
                        verb = "|".join(str(getattr(e, "value", "")) for e in kw.value.elts)
            return verb, d.args[0].value
    return None


_FROM = re.compile(r"^[ \t]*from[ \t]+(\.*)([\w.]*)[ \t]+import[ \t]+(\([^)]*\)|[^\n]+)", re.M)
_IMPORT = re.compile(r"^[ \t]*import[ \t]+([\w.]+(?:[ \t]+as[ \t]+\w+)?(?:[ \t]*,[ \t]*[\w.]+(?:[ \t]+as[ \t]+\w+)?)*)", re.M)


def _regex_imports(text):
    """Imports for a file `ast` could not parse, in the same shape the real parser produces."""
    out = []
    for m in _FROM.finditer(text):
        body = re.sub(r"#[^\n]*", "", m.group(3)).strip("() \t\n")
        names = [n.split()[0] for n in body.split(",") if n.strip()]
        out.append((len(m.group(1)), m.group(2), names, True))
    for m in _IMPORT.finditer(text):
        out += [(0, part.split()[0], [], False) for part in m.group(1).split(",")]
    return out


class Python(Language):
    name = "Python"
    extensions = (".py",)
    interpreters = ("python", "python3", "python2")
    color = "#3572A5"
    manifests = ("pyproject.toml", "__init__.py")   # __init__.py files mark package roots, even empty ones
    regex_routes = False                            # routes come from decorators (see _route)
    header_comments = False                         # module docstrings are used instead
    entry_by_name = True
    implicit = "none"                               # every use is an import
    stem_layers = {"view": "api", "views": "api"}   # Django-style views.py / views/ is the request layer

    # ---- parsing
    def parse(self, f):
        text = f["text"]
        f["entry"] = (f["path"].endswith("__main__.py")   # what `python -m package` runs
                      or bool(re.search(r"^if\s+__name__\s*==\s*['\"]__main__['\"]", text, re.M)))
        try:
            tree = ast.parse(text)
        except (SyntaxError, ValueError, RecursionError):
            # Python 2 code, or syntax newer than the Python running repoatlas: read it approximately
            f["imports"] = _regex_imports(text)
            return generic_symbols(f)
        f["doc"] = first_sentence(ast.get_docstring(tree) or "")
        for node in tree.body:
            if isinstance(node, ast.ClassDef):
                bases = [b.id if isinstance(b, ast.Name) else getattr(b, "attr", "") for b in node.bases]
                members = [n.name for n in node.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and not n.name.startswith("_")]
                f["symbols"].append(dict(k="class", n=node.name, sig="(" + ", ".join(b for b in bases if b) + ")" if bases else "",
                                         doc=first_sentence(ast.get_docstring(node) or ""), line=node.lineno, members=members[:12]))
            elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                r = _route(node)
                f["symbols"].append(dict(k="route" if r else "function", n=r[1] if r else node.name,
                                         sig=r[0] if r else _signature(node), doc=first_sentence(ast.get_docstring(node) or ""), line=node.lineno))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                f["imports"] += [(0, a.name, [], False) for a in node.names]
            elif isinstance(node, ast.ImportFrom):
                f["imports"].append((0 if node.level is None else node.level, node.module or "", [a.name for a in node.names], True))
        f["symbols"].sort(key=lambda s: s["n"].startswith("_") if s["k"] != "route" else False)

    # ---- import resolution
    def prepare(self, project):
        inits = {posixpath.dirname(p) for p in project.man.get("__init__.py", [])}
        roots = {""}
        for d in inits:                         # the folder above the top-most package is an import root
            top = d
            while top and posixpath.dirname(top) in inits:
                top = posixpath.dirname(top)
            roots.add(posixpath.dirname(top))
        roots |= {r for r in ("src", "lib") if r in project.dirs}
        self.roots = sorted(roots, key=lambda r: (r != "", r))
        for rel in project.man.get("pyproject.toml", []):          # console_scripts are entry points
            for sec in re.findall(r"\[(?:project|tool\.poetry)\.scripts\]\s*\n(.*?)(?:\n\[|\Z)", project.read(rel), re.S):
                for mod in re.findall(r'=\s*"([\w.]+):[\w.]+"', sec):
                    h = self._file(project, mod.split("."), self.roots)
                    if h:
                        project.declared.append(h)

    @staticmethod
    def _file(project, parts, roots):
        for r in roots:
            base = "/".join(([r] if r else []) + list(parts))
            for c in ((base + ".py") if base else None, (base + "/" if base else "") + "__init__.py"):
                if c and c in project.paths:
                    return c
        return None

    def resolve(self, project, f):
        uses, ext = set(), set()
        d = posixpath.dirname(f["path"])
        for level, mod, names, is_from in f["imports"]:
            parts = mod.split(".") if mod else []
            if level:                            # from . import x  /  from ..pkg import y
                base = d.split("/") if d else []
                base = base[: max(0, len(base) - (level - 1))]
                roots, parts = [""], base + parts
            else:
                roots = self.roots
            hits = []
            if is_from:
                hits = [h for h in (self._file(project, parts + [n], roots) for n in names if n != "*") if h]
                if len(hits) < len(names) or not names:
                    b = self._file(project, parts, roots)
                    if b:
                        hits.append(b)
            else:
                for k in range(len(parts), 0, -1):
                    h = self._file(project, parts[:k], roots)
                    if h:
                        hits.append(h)
                        break
            if hits:
                uses.update(hits)
            elif not level and parts:
                if parts[0] in STDLIB:
                    continue
                sib = self._file(project, parts, [d]) if d else None   # script importing a sibling file
                if sib:
                    uses.add(sib)
                else:
                    ext.add((parts[0], mod))
        for h in list(uses):   # importing a.b.c runs a/__init__.py and a/b/__init__.py first
            pkg = posixpath.dirname(h)
            while pkg and pkg + "/__init__.py" in project.paths and not d.startswith(pkg):
                uses.add(pkg + "/__init__.py")
                pkg = posixpath.dirname(pkg)
        return uses, ext
