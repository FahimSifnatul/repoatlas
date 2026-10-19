"""Shared building blocks for language plug-ins.

Adding a language = drop a new file in this folder that defines a `Language` subclass.
It is discovered automatically; nothing else needs editing.

What a language file provides
-----------------------------
    name / extensions / color   who it is, which files it owns, its colour in the GUI
    parse(f)                    fill f["symbols"], f["imports"], f["doc"], f["entry"]
    prepare(project)            (optional) build indexes once, e.g. read go.mod
    resolve(project, f)         turn f["imports"] into (files inside the repo, outside libraries)
"""
from __future__ import annotations

import posixpath
import re
from pathlib import Path

ROUTE_RES = [
    re.compile(r"\b(?:app|router|routes?)\.(get|post|put|delete|patch|all)\(\s*['\"`]([^'\"`]+)['\"`]"),
    re.compile(r"@(Get|Post|Put|Delete|Patch)Mapping\(\s*(?:value\s*=\s*|path\s*=\s*)?\"([^\"]*)\""),
    re.compile(r"\.(GET|POST|PUT|DELETE|PATCH)\(\s*\"([^\"]+)\""),
    re.compile(r"HandleFunc\(\s*\"([^\"]+)\""),
]
DIRECTIVE = re.compile(r"(eslint|tslint|prettier|jshint|istanbul|@ts-|@flow|global |noqa|pylint|type:|use strict|vim:|-\*-)", re.I)
LICENSE = re.compile(r"copyright|licen[sc]e|permission is hereby|spdx|all rights reserved|warranty", re.I)
GENERIC_SYM = re.compile(r"^\s*(?:(?:public|private|protected|internal|static|abstract|final|export|async|pub)\s+)*"
                         r"(class|interface|struct|enum|trait|module|def|function|fn|func)\s+(\w+)", re.M)


# ───────────────────────── text helpers ─────────────────────────

def trim(s: str, n: int = 150) -> str:
    s = re.sub(r"\s+", " ", s or "").strip()
    if len(s) <= n:
        return s
    cut = s[:n]
    m = max(cut.rfind(". "), cut.rfind("! "), cut.rfind("? "))
    return cut[: m + 1] if m > n * 0.5 else cut.rsplit(" ", 1)[0] + "…"


def first_sentence(s: str, n: int = 150) -> str:
    s = re.sub(r"\s+", " ", s or "").strip()
    m = re.match(r"(.+?[.!?])(\s|$)", s)
    return trim(m.group(1) if m else s, n)


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


def lead_comment(lines: list, i: int) -> str:
    """Comment block directly above line i (0-based), for any C-like / hash-comment language."""
    out, j = [], i - 1
    while j >= 0 and len(out) < 6:
        s = lines[j].strip()
        is_c = re.match(r"^(//+|/\*+|\*+/?|--)", s) or s.endswith("*/")
        is_h = s.startswith("#") and not re.match(r"#\s*(include|define|if|ifdef|ifndef|endif|else|pragma|import|undef)|#[\[!]", s)
        if not (is_c or is_h):
            break
        out.append(re.sub(r"^(//+!?|#+|/\*+|\*+/?|--)\s?|\*/$", "", s).strip())
        j -= 1
    text = " ".join(t for t in reversed(out) if t and not t.startswith("@"))
    return first_sentence(text)


def header_doc(text: str) -> str:
    """First sentence of the comment block at the very top of a file (skips licences and linter directives)."""
    t = re.sub(r"^\ufeff?(#!.*\n)?", "", text, count=1)
    t = re.sub(r"^\s*(['\"])use strict\1;?\s*", "", t)
    m = re.match(r"\s*/\*+(.*?)\*/", t, re.S)
    if m:
        body = m.group(1)
    else:
        block = []
        for ln in t.lstrip().splitlines():
            s = ln.strip()
            if s.startswith("//") or (s.startswith("#") and not re.match(r"#\s*(include|define|if|pragma|import)|#[\[!]", s)):
                block.append(re.sub(r"^(//+!?|#+)\s?", "", s))
            else:
                break
        body = "\n".join(block)
    if not body or LICENSE.search(body) or DIRECTIVE.match(body.strip().lstrip("*! \n")):
        return ""
    lines = [re.sub(r"^\s*\*+\s?", "", l).strip() for l in body.splitlines()]
    lines = [l for l in lines if l and not l.startswith("@")]
    return first_sentence(" ".join(lines))


def add_routes(f: dict) -> None:
    """Spot web endpoints written as app.get("/x"), @GetMapping("/x"), r.GET("/x") or HandleFunc("/x")."""
    for rx in ROUTE_RES:
        for m in rx.finditer(f["text"]):
            g = m.groups()
            verb, path = (g[0].upper(), g[1]) if len(g) == 2 else ("ANY", g[0])
            if path.startswith("/"):
                f["symbols"].append(dict(k="route", n=path, sig=verb, doc="", line=line_of(f["text"], m.start())))


def generic_symbols(f: dict) -> None:
    """Best-effort classes and functions for any C-like or Ruby-like language."""
    lines = f["text"].splitlines()
    for m in GENERIC_SYM.finditer(f["text"]):
        ln = line_of(f["text"], m.start())
        kind = "class" if m.group(1) in ("class", "interface", "struct", "enum", "trait", "module") else "function"
        f["symbols"].append(dict(k=kind, n=m.group(2), sig="", doc=lead_comment(lines, ln - 1), line=ln))


# ───────────────────────── plug-in interface ─────────────────────────

class Project:
    """Read-only view of the repository that languages use while resolving imports."""

    def __init__(self, root: Path, files: dict, man: dict):
        self.root, self.files, self.man = root, files, man   # man: manifest name -> [relative paths]
        self.paths = set(files)
        self.dirs = set()
        for p in files:
            d = posixpath.dirname(p)
            while d:
                self.dirs.add(d)
                d = posixpath.dirname(d)
        self.declared: list = []   # files that package.json / pyproject etc. name as entry points
        self._shared: dict = {}

    def read(self, rel: str) -> str:
        try:
            return (self.root / rel).read_text(encoding="utf-8", errors="ignore")
        except OSError:
            return ""

    def shared(self, key: str, factory):
        """Build an index once per project, even when several languages (JS + TS + Vue...) need it."""
        if key not in self._shared:
            self._shared[key] = factory(self)
        return self._shared[key]


class Language:
    name = ""                  # shown in the GUI and used as the key everywhere
    extensions: tuple = ()     # e.g. (".py",)
    color = "#7d8aa5"          # language bar colour
    manifests: tuple = ()      # file names the scanner should remember: ("go.mod",)
    skip_suffixes: tuple = ()  # files to ignore even though the extension matches: (".d.ts",)
    regex_routes = True        # look for app.get("/x")-style endpoints
    header_comments = True     # use the comment at the top of a file as its description
    entry_by_name = False      # main.x / index.x / app.x may be a program entry point
    default_layer = None       # e.g. "interface" for HTML and CSS
    stem_layers: dict = {}     # file-name words with a special meaning in this language

    def analyze(self, f: dict) -> None:
        self.parse(f)
        if self.regex_routes:
            add_routes(f)
        if not f["doc"] and self.header_comments:
            f["doc"] = header_doc(f["text"])
        f["symbols"] = f["symbols"][:60]

    def parse(self, f: dict) -> None:
        generic_symbols(f)

    def prepare(self, project: Project) -> None:
        pass

    def resolve(self, project: Project, f: dict):
        return set(), set()

    def layer_hint(self, f: dict):
        return self.default_layer
