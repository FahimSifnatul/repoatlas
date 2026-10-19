"""
The analysis pipeline: scan -> parse -> resolve imports -> classify layers -> group into parts
-> build tree + dependency edges -> find entry points / flow -> render one HTML file.

Everything language-specific lives in languages/ (one file per language: python.py, go.py, rust.py ...).
This module only knows the language-neutral pipeline. The command line lives in cli.py.
"""
from __future__ import annotations

import json
import os
import pkgutil
import posixpath
import re
import sys
from collections import Counter, defaultdict
from datetime import date
from pathlib import Path

from . import languages
from .languages import Project, trim

# ════════════════════════════════ configuration ════════════════════════════════

SKIP_DIRS = set(
    "node_modules vendor third_party site-packages dist build out target obj __pycache__ venv "
    "virtualenv coverage htmlcov docs documentation site storybook-static".split()
)
MAX_BYTES = 400_000

# key, display name, plain-English tagline, colour, group
LAYERS = [
    ("interface", "Interface", "What people see and touch: screens, pages and commands.", "#e0457b", "stack"),
    ("api", "API & Routing", "The front door: receives requests and sends them to the right place.", "#f08a24", "stack"),
    ("logic", "Core Logic", "The brain: the rules and steps that make things happen.", "#4f5bd5", "stack"),
    ("data", "Data & Storage", "The memory: models, databases and saved information.", "#1fa971", "stack"),
    ("integration", "Integrations", "The phone lines: talks to outside services.", "#17a2b8", "stack"),
    ("shared", "Shared Toolkit", "Small helpers that the rest of the project reuses.", "#8e6bd8", "stack"),
    ("config", "Config & Tooling", "Settings, build scripts and deployment.", "#6b7a90", "support"),
    ("tests", "Tests & Examples", "Checks that it works, and samples of how to use it.", "#c9961a", "support"),
]
SHORT = {
    "interface": "Screens and commands people interact with",
    "api": "Receives requests and routes them",
    "logic": "Core rules and processes",
    "data": "Data models and storage",
    "integration": "Talks to outside services",
    "shared": "Reusable helper code",
    "config": "Settings and tooling",
    "tests": "Tests and examples",
}

# words in file / folder names that reveal a file's role (first layer listed wins on conflicts)
TOKENS = [
    ("tests", "test tests spec specs e2e testing fixtures mocks mock cypress playwright example examples "
              "demo demos sample samples benchmark benchmarks bench"),
    ("config", "config configs conf settings setup env deploy deployment infra infrastructure terraform k8s "
               "kubernetes helm docker ci scripts script build webpack vite rollup babel eslint jest tailwind "
               "postcss gulpfile gruntfile"),
    ("interface", "ui components component pages page views view templates template static public assets "
                  "frontend web screens screen layouts layout widgets widget styles style css cli cmd commands "
                  "command console gui tui forms form theme themes admin"),
    ("api", "api apis routes route router routers controllers controller handlers handler endpoints endpoint "
            "resolvers resolver graphql rest grpc middleware middlewares server urls rpc webhook webhooks "
            "serializers serializer wsgi asgi"),
    ("logic", "services service core domain usecases usecase logic engine managers manager business workflows "
              "workflow tasks task jobs job workers worker processors processor pipeline pipelines signals"),
    ("data", "models model db database databases repository repositories repo repos dao entities entity schema "
             "schemas migrations migration orm store stores storage prisma sql queries query dal persistence cache"),
    ("integration", "clients client adapters adapter integrations integration gateway gateways connectors "
                    "connector sdk drivers driver external"),
    ("shared", "utils util utilities helpers helper common shared support misc constants types typing compat"),
]
TOKEN_LAYER: dict = {}
for _layer, _words in TOKENS:
    for _w in _words.split():
        TOKEN_LAYER.setdefault(_w, _layer)

# well-known libraries -> what kind of thing they are (used for layer hints and the "Built with" list)
EXT_CATS = {
    "web": ("Web framework", "flask fastapi django starlette aiohttp tornado sanic bottle werkzeug express koa "
            "fastify hapi next nuxt @nestjs/core @nestjs/common github.com/gin-gonic/gin github.com/gorilla/mux "
            "github.com/labstack/echo github.com/gofiber/fiber net/http org.springframework.web "
            "org.springframework.boot actix_web axum rocket warp"),
    "db": ("Database", "sqlalchemy psycopg2 psycopg asyncpg pymysql pymongo motor peewee tortoise sqlite3 mongoose "
           "mongodb sequelize typeorm prisma @prisma/client knex pg mysql mysql2 better-sqlite3 redis ioredis "
           "gorm.io/gorm github.com/jmoiron/sqlx database/sql github.com/redis/go-redis github.com/lib/pq "
           "github.com/jackc/pgx org.hibernate javax.persistence jakarta.persistence org.springframework.data "
           "diesel sqlx rusqlite elasticsearch cassandra"),
    "ui": ("UI framework", "react react-dom vue svelte @angular/core preact solid-js lit tkinter pyqt5 pyqt6 "
           "pyside2 pyside6 streamlit gradio kivy jquery @mui/material antd"),
    "queue": ("Messaging & jobs", "celery rq dramatiq kafka confluent_kafka kafkajs amqplib pika bull bullmq nats"),
    "cloud": ("Cloud & SaaS", "boto3 botocore aws-sdk @aws-sdk google.cloud @google-cloud firebase firebase-admin "
              "supabase azure stripe twilio sendgrid slack_sdk @slack"),
    "http": ("HTTP clients", "requests httpx urllib3 axios node-fetch got superagent reqwest okhttp retrofit"),
    "ai": ("AI & data science", "openai anthropic langchain transformers torch tensorflow sklearn keras numpy "
           "pandas scipy xgboost"),
}
EXT_CAT = {k: cat for cat, (_, words) in EXT_CATS.items() for k in words.split()}
CAT_LAYER = [("ui", "interface"), ("web", "api"), ("db", "data"), ("http", "integration"),
             ("cloud", "integration"), ("queue", "integration")]

ENTRY_STEMS = {"main", "__main__", "manage", "wsgi", "asgi", "server", "app", "index", "cli", "run", "start"}

ROUTE_HINT = re.compile(
    r"@(?:\w+\.)?(?:route|get|post|put|delete|patch)\(|\b(?:app|router)\.(?:get|post|put|delete|patch|use)\(|"
    r"@RestController|@Controller|@(?:Get|Post|Put|Delete|Patch|Request)Mapping|HandleFunc\(|urlpatterns"
)


# ════════════════════════════════ small helpers ════════════════════════════════

def toks(s: str) -> list:
    return [t for t in re.split(r"[^a-z0-9]+", re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", s).lower()) if t]


def cat_of(spec: str):
    s = spec.lower()
    while s:
        if s in EXT_CAT:
            return EXT_CAT[s]
        cut = max(s.rfind("."), s.rfind("/"))
        if cut <= 0:
            return None
        s = s[:cut]
    return None


# ════════════════════════════════ 1. scan ════════════════════════════════

def scan(root: Path):
    files, man = {}, defaultdict(list)
    top = os.path.realpath(root).rstrip(os.sep) + os.sep
    for dp, dns, fns in os.walk(root):
        dns[:] = sorted(
            d for d in dns
            if d not in SKIP_DIRS and not d.startswith(".") and not os.path.exists(os.path.join(dp, d, "pyvenv.cfg"))
        )
        rd = os.path.relpath(dp, root).replace(os.sep, "/")
        rd = "" if rd == "." else rd
        for fn in sorted(fns):
            rel = f"{rd}/{fn}" if rd else fn
            p = os.path.join(dp, fn)
            # never read a pipe / device, or follow a symlink out of the repo (it could point at ~/.aws or /proc)
            if not os.path.isfile(p) or (os.path.islink(p) and not os.path.realpath(p).startswith(top)):
                continue
            if fn in languages.MANIFESTS:
                man[fn].append(rel)
            if fn.lower().startswith("readme"):
                man["README"].append(rel)
            plugin = languages.BY_EXT.get(os.path.splitext(fn)[1].lower())
            if not plugin or ".min." in fn or fn.endswith(plugin.skip_suffixes):
                continue
            lang = plugin.name
            try:
                if os.path.getsize(p) > MAX_BYTES:
                    continue
                with open(p, encoding="utf-8", errors="ignore") as fh:
                    text = fh.read()
            except OSError:
                continue
            loc = sum(1 for l in text.splitlines() if l.strip())
            if loc:
                files[rel] = dict(path=rel, lang=lang, text=text, loc=loc, layer=None,
                                  doc="", symbols=[], imports=[], mods=[], pkg="", entry=False)
    return files, man


# ════════════════════════════════ 4. classify into layers ════════════════════════════════

def classify(f, fullstack: bool, plugin):
    parts = f["path"].split("/")
    segs = [parts[-1].rsplit(".", 1)[0]] + parts[-2::-1]  # file stem first, then folders deepest -> shallowest
    all_toks = [toks(s) for s in segs]
    if any(t in TOKEN_LAYER and TOKEN_LAYER[t] == "tests" for ts in all_toks for t in ts):
        return "tests"
    for i, ts in enumerate(all_toks):
        for t in reversed(ts):
            ly = TOKEN_LAYER.get(t)
            if ly:
                if t == "client":
                    return "interface" if fullstack else "integration"
                if i == 0 and t in plugin.stem_layers:
                    return plugin.stem_layers[t]
                return ly
    cats = {cat_of(spec) for _, spec in f["ext"]}
    for cat, layer in CAT_LAYER:
        if cat in cats:
            return layer
    hint = plugin.layer_hint(f)
    if hint:
        return hint
    if ROUTE_HINT.search(f["text"]):
        return "api"
    return None


# ════════════════════════════════ 5. group files into "parts" (modules) ════════════════════════════════

def build_units(F, small=False):
    tree, loose, sub = defaultdict(set), defaultdict(list), defaultdict(list)
    for p in F:
        d = posixpath.dirname(p)
        loose[d].append(p)
        parts = d.split("/") if d else []
        for i in range(len(parts) + 1):
            anc = "/".join(parts[:i])
            sub[anc].append(p)
            if i:
                tree["/".join(parts[: i - 1])].add(anc)

    def squash(d):  # skip wrapper folders that contain just one sub-folder
        while not loose.get(d) and len(tree.get(d, ())) == 1:
            d = next(iter(tree[d]))
        return d

    def dominant(d):
        c = Counter()
        for p in sub[d]:
            if F[p]["layer"]:
                c[F[p]["layer"]] += F[p]["loc"]
        total = sum(F[p]["loc"] for p in sub[d]) or 1
        if not c:
            return None, 0.0
        k, v = c.most_common(1)[0]
        return k, v / total

    units = []

    def visit(d, depth):
        d = squash(d)
        subs, lo, n = sorted(tree.get(d, ())), loose.get(d, []), len(sub[d])
        top, share = dominant(d)
        calm = top in ("tests", "config") and share >= 0.7  # keep test/config folders as one box
        expand = depth == 0 or (not calm and depth < 3 and (
            (small and n >= 2) or (bool(subs) and n > 8 and (len(subs) >= 3 or share < 0.7))))
        if not expand:
            units.append(dict(kind="dir", id=d, path=d, files=sub[d]))
            return
        for s in subs:
            visit(s, depth + 1)
        groups = defaultdict(list)
        for p in lo:
            groups[F[p]["layer"] or "logic"].append(p)
        for ly, ps in groups.items():
            if len(ps) <= 3 or len(lo) <= 24:
                units.extend(dict(kind="file", id=p, path=p, files=[p]) for p in ps)
            else:
                units.append(dict(kind="group", id=f"{d}#{ly}", path=d, files=ps, layer=ly, multi=len(groups) > 1))

    visit("", 0)
    if not small and len(units) < 8:  # tiny repo: open the folders up so there is something to explore
        return build_units(F, small=True)
    return units, tree, loose, sub, squash


# ════════════════════════════════ 6. assemble the model ════════════════════════════════

def read_para(path: Path) -> str:
    try:
        txt = path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return ""
    for para in re.split(r"\n\s*\n", txt):
        lines = [l.strip() for l in para.strip().splitlines()]
        if not lines or lines[0].startswith(("#", "=", "-", "!", "[![", "<", "|", "```", ">", ":", "..")):
            continue
        if any(len(l) > 2 and set(l) <= set("=-~#") for l in lines):
            continue
        s = " ".join(lines)
        s = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", s)
        s = re.sub(r"<[^>]+>|[`*]", "", s)
        if len(s) > 25:
            return trim(s, 260)
    return ""


def analyze(root: Path, name: str):
    files, man = scan(root)
    if not files:
        sys.exit("repoatlas: no source files found in " + str(root))
    plugins = {c.name: c() for c in languages.ALL}                 # one fresh instance per language per run
    for f in files.values():
        plugins[f["lang"]].analyze(f)                              # symbols, imports, doc comment
    project = Project(root, files, man)
    for name_ in sorted({f["lang"] for f in files.values()}):
        plugins[name_].prepare(project)                            # read go.mod, package.json ...
    for p, f in files.items():
        uses, ext = plugins[f["lang"]].resolve(project, f)
        f["uses"], f["ext"] = sorted(uses - {p}), sorted(ext)   # sorted: identical output on every run
    top_dirs = {p.split("/")[0].lower() for p in files if "/" in p}
    fullstack = bool(top_dirs & {"client", "frontend", "web"}) and bool(top_dirs & {"server", "backend", "api"})
    for f in files.values():
        f["layer"] = classify(f, fullstack, plugins[f["lang"]])
    F = files

    # ---- parts (units) and layer assignment
    units, tree, loose, sub, squash = build_units(F)
    layer_name = {k: n for k, n, *_ in LAYERS}
    unit_of, ulayer = {}, {}
    for u in units:
        c = Counter()
        for p in u["files"]:
            if F[p]["layer"]:
                c[F[p]["layer"]] += F[p]["loc"]
        ulayer[u["id"]] = u.get("layer") or (c.most_common(1)[0][0] if c else "logic")
        for p in u["files"]:
            F[p]["layer"] = F[p]["layer"] or ulayer[u["id"]]
            unit_of[p] = u["id"]

    # ---- dependency edges between files
    fedges = Counter((p, q) for p, f in F.items() for q in f["uses"] if q in F)

    # ---- entry points: declared by tooling > explicit main() > conventional file names
    declared = {h for h in project.declared if h in F}
    imported = {q for f in F.values() if f["layer"] != "tests" for q in f["uses"]}
    cand = []
    for p, f in F.items():
        if f["layer"] == "tests":
            continue
        stem = posixpath.basename(p).rsplit(".", 1)[0]
        if p in declared:
            pr = 0
        elif f["entry"]:
            pr = 1
        elif stem in ENTRY_STEMS and p.count("/") <= 2 and plugins[f["lang"]].entry_by_name and p not in imported:
            pr = 2
        else:
            continue
        cand.append(((pr, p.count("/"), -f["loc"]), p))
    cand.sort()
    pool = [p for _, p in cand if F[p]["layer"] != "config"] or [p for _, p in cand]
    entries, entry_kind = pool[:5], "Entry points"
    if not entries:  # a library: its "front door" is the package's top-level index
        lib = [p for p in F if posixpath.basename(p).rsplit(".", 1)[0] in ("__init__", "index", "lib", "mod")
               and F[p]["layer"] != "tests" and p.count("/") <= 3]
        if lib:
            m = min(p.count("/") for p in lib)
            entries = sorted((p for p in lib if p.count("/") == m), key=lambda p: -len(F[p]["uses"]))[:3]
            entry_kind = "Public API"
    entry_set = set(entries)

    # ---- node tree: layer > part > folder > file
    nodes = {}

    def mk(id_, name_, kind, path, fl, parent):
        langs = Counter()
        for p in fl:
            langs[F[p]["lang"]] += F[p]["loc"]
        n = dict(id=id_, name=name_, kind=kind, path=path, files=len(fl), loc=sum(F[p]["loc"] for p in fl),
                 langs=[[l, c] for l, c in langs.most_common(4)], children=[], edges=[], uses=[], usedBy=[],
                 parent=parent, layer=None, desc="", _files=fl)
        nodes[id_] = n
        return n

    def file_node(p, parent):
        f = F[p]
        n = mk(p, posixpath.basename(p), "file", p, [p], parent)
        n.update(layer=f["layer"], lang=f["lang"], symbols=f["symbols"], entry=p in entry_set,
                 ext=sorted({nm for nm, _ in f["ext"]})[:12])
        return n

    def dir_node(d, parent, label):
        n = mk(d, label, "dir", d, sub[d], parent)
        for s in sorted(tree.get(d, ())):
            s2 = squash(s)
            n["children"].append(dir_node(s2, d, s2[len(d) + 1:] if d else s2)["id"])
        for p in sorted(loose.get(d, ())):
            n["children"].append(file_node(p, d)["id"])
        return n

    generic = {"src", "lib", "app", "main", "core", "java", "python", "source", "pkg", "internal", "cmd"}

    def unit_name(u):
        if u["kind"] == "file":
            return posixpath.basename(u["id"])
        d = u["path"]
        base = posixpath.basename(d) or name
        if u["kind"] == "dir":
            if base in generic:
                anc = [a for a in d.split("/")[:-1] if a not in generic]
                if anc:
                    base = anc[-1] + "/" + base
            return base
        label = base if d else "project root"
        return f"{label} files" if not u["multi"] else f"{label} ({layer_name[u['layer']]})"

    layer_units = defaultdict(list)
    for u in units:
        lk = ulayer[u["id"]]
        lid = "layer:" + lk
        if u["kind"] == "file":
            n = file_node(u["id"], lid)
        elif u["kind"] == "dir":
            n = dir_node(u["id"], lid, unit_name(u))
        else:
            n = mk(u["id"], unit_name(u), "dir", u["path"], u["files"], lid)
            n["children"] = [file_node(p, u["id"])["id"] for p in sorted(u["files"])]
        n["unit"] = True
        n["layer"] = lk
        layer_units[lk].append(n["id"])
    for n in nodes.values():  # folders inside a part take the layer that dominates them
        if n["kind"] == "dir" and not n.get("unit"):
            c = Counter()
            for p in n["_files"]:
                c[F[p]["layer"]] += F[p]["loc"]
            n["layer"] = c.most_common(1)[0][0]

    # ---- dependency edges between siblings (drawn as lines in the GUI)
    for n in list(nodes.values()):
        if not n["children"]:
            continue
        owner = {p: c for c in n["children"] for p in nodes[c]["_files"]}
        cnt = Counter()
        for p in n["_files"]:
            for q in F[p]["uses"]:
                a, b = owner.get(p), owner.get(q)
                if a and b and a != b:
                    cnt[(a, b)] += 1
        n["edges"] = [[a, b, w] for (a, b), w in cnt.most_common(80)]

    uedges = Counter()
    for (p, q), w in fedges.items():
        if unit_of[p] != unit_of[q]:
            uedges[(unit_of[p], unit_of[q])] += 1
    for (p, q), w in fedges.items():
        nodes[p]["uses"].append((q, w))
        nodes[q]["usedBy"].append((p, w))
    for (a, b), w in uedges.items():
        if nodes[a]["kind"] != "file":
            nodes[a]["uses"].append((b, w))
        if nodes[b]["kind"] != "file":
            nodes[b]["usedBy"].append((a, w))
    for n in nodes.values():
        n["uses"] = [i for i, _ in sorted(n["uses"], key=lambda x: -x[1])]
        n["usedBy"] = [i for i, _ in sorted(n["usedBy"], key=lambda x: -x[1])]

    # ---- layers
    layers_out, luse = [], defaultdict(Counter)
    for (a, b), w in uedges.items():
        if ulayer[a] != ulayer[b]:
            luse[ulayer[a]][ulayer[b]] += w
    for key, nm, tag, color, grp in LAYERS:
        us = layer_units.get(key)
        if not us:
            continue
        us.sort(key=lambda i: -nodes[i]["loc"])
        fl = [p for i in us for p in nodes[i]["_files"]]
        ln = mk("layer:" + key, nm, "layer", "", fl, None)
        inside = set(us)
        ln.update(layer=key, children=us, desc=tag,
                  edges=[[a, b, w] for (a, b), w in uedges.most_common() if a in inside and b in inside][:80])
        layers_out.append(dict(key=key, name=nm, tag=tag, color=color, group=grp, units=us, files=len(fl),
                               loc=ln["loc"], uses=[k for k, _ in luse[key].most_common(3)]))

    # ---- plain-English descriptions
    readme_dir = {posixpath.dirname(r): r for r in reversed(man.get("README", []))}

    def describe(n):
        if n["kind"] == "layer":
            return n["desc"]
        if n["kind"] == "file":
            f = F[n["id"]]
            if f["doc"]:
                return f["doc"]
            names = [s["n"] for s in f["symbols"] if s["k"] in ("class", "function", "component", "type") and not s["n"].startswith("_")]
            if f["layer"] == "tests" and not names:
                return "Test code."
            if names:
                return "Defines " + ", ".join(names[:3]) + ("…" if len(names) > 3 else ".")
            routes = [s for s in f["symbols"] if s["k"] == "route"]
            if routes:
                return f"Handles {len(routes)} web endpoint{'s' if len(routes) != 1 else ''}."
            return f"{f['lang']} file, {f['loc']} lines."
        if n["kind"] == "dir" and n["id"] in readme_dir:
            t = read_para(root / readme_dir[n["id"]])
            if t:
                return t
        for stem in ("__init__", "index", "mod", "lib", "doc", "main"):
            for p in loose.get(n["id"], []) if n["kind"] == "dir" else []:
                if posixpath.basename(p).rsplit(".", 1)[0] == stem and F[p]["doc"]:
                    return F[p]["doc"]
        top = [posixpath.basename(p).rsplit(".", 1)[0] for p in sorted(n["_files"], key=lambda p: -F[p]["loc"])
               if posixpath.basename(p).rsplit(".", 1)[0] not in ("__init__", "index", "mod")][:3]
        s = f"{SHORT.get(n['layer'], '')}. {n['files']} file{'s' if n['files'] != 1 else ''}"
        return s + (", including " + ", ".join(top) + "." if top else ".")

    for n in nodes.values():
        n["desc"] = describe(n)

    # ---- flow: which parts does the program touch, in what order?
    flow = None
    if entries:
        adj = defaultdict(dict)
        for (a, b), w in uedges.items():
            adj[a][b] = w
        start = list(dict.fromkeys(unit_of[e] for e in entries))
        skip = {u for u in ulayer if ulayer[u] in ("tests", "config")} - set(start)
        dist, queue = {u: 0 for u in start}, list(start)
        for u in queue:
            for v in adj[u]:
                if v not in dist and v not in skip:
                    dist[v] = dist[u] + 1
                    queue.append(v)
        by_stage = defaultdict(list)
        for u, d in dist.items():
            by_stage[min(d, 4)].append(u)
        stages = []
        for s in sorted(by_stage):
            prev = stages[-1]["all"] if stages else []
            us = sorted(by_stage[s], key=lambda u: (-sum(adj[p].get(u, 0) for p in prev), -nodes[u]["loc"]))
            stages.append(dict(units=us[:7], more=max(0, len(us) - 7), all=us))
        shown = {u: i for i, st in enumerate(stages) for u in st["units"]}
        fedges_out = [[a, b, w] for (a, b), w in uedges.items() if a in shown and b in shown and shown[b] > shown[a]]
        names = lambda st: ", ".join(nodes[u]["name"] for u in st["units"][:3])
        text = ("It starts in " if entry_kind == "Entry points" else "People using this library start from ") + names(stages[0])
        if len(stages) > 1:
            text += f", which calls into {names(stages[1])}"
        if len(stages) > 2:
            text += f", and then relies on {names(stages[2])}"
        flow = dict(kind=entry_kind, narrative=text + ".", entries=entries,
                    stages=[dict(units=s["units"], more=s["more"]) for s in stages], edges=fedges_out)

    # ---- repository summary
    ext_files, ext_spec = defaultdict(set), {}
    for p, f in F.items():
        for nm, spec in f["ext"]:
            ext_files[nm].add(p)
            ext_spec.setdefault(nm, spec)
    ranked = sorted(ext_files, key=lambda k: (-len(ext_files[k]), k))
    catnames = defaultdict(list)
    for nm in ranked:
        c = cat_of(ext_spec[nm]) or cat_of(nm)
        if c:
            catnames[c].append(nm)
    stack = [dict(label=lab, items=catnames[c][:6]) for c, (lab, _) in EXT_CATS.items() if catnames.get(c)]
    libs = [nm for nm in ranked if not (cat_of(ext_spec[nm]) or cat_of(nm))][:10]
    cats = set(catnames)
    cli_hint = any(re.search(r"\b(argparse|click|typer|commander|yargs|cobra|clap|optparse)\b", F[p]["text"]) for p in entries)
    if entry_kind == "Public API":
        kind = "library"
    elif "web" in cats and "ui" in cats:
        kind = "full-stack app"
    elif "web" in cats:
        kind = "backend service"
    elif "ui" in cats:
        kind = "frontend app"
    else:
        kind = "command-line tool" if cli_hint else "application"
    langs = Counter()
    for f in F.values():
        langs[f["lang"]] += f["loc"]
    main_lang = langs.most_common(1)[0][0]
    summary = ""
    if "" in readme_dir:
        summary = read_para(root / readme_dir[""])
    if not summary:
        summary = f"A {main_lang} {kind} with {len(units)} main parts arranged in {len(layers_out)} layers."
    repo = dict(name=name, summary=summary, kind=f"{main_lang} {kind}", files=len(F), loc=sum(langs.values()),
                parts=len(units), deps=len(ext_files), langs=[[l, c] for l, c in langs.most_common(6)],
                routes=sum(1 for f in F.values() if f["layer"] != "tests" for s in f["symbols"] if s["k"] == "route"),
                stack=stack, libs=libs, entries=entries, generated=date.today().isoformat(),
                colors={n: plugins[n].color for n in langs})

    for n in nodes.values():
        n.pop("_files", None)
    return dict(repo=repo, layers=layers_out, nodes=nodes, flow=flow)


# ════════════════════════════════ 7. render ════════════════════════════════

def render(data: dict, title: str) -> str:
    import html as _html
    tpl = pkgutil.get_data(__package__, "viewer.html").decode("utf-8")
    # every "<" escaped: "</script>" or "<!--<script" in a file name can then never end or derail the <script> block
    blob = json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("<", "\\u003c")
    return tpl.replace("__TITLE__", _html.escape(title)).replace("/*__DATA__*/null", blob)
