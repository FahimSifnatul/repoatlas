"""
The analysis pipeline: scan -> parse -> resolve imports + link paths and names -> classify layers -> group into
parts -> build tree + dependency edges -> find entry points / flow / inputs and outputs -> render one HTML file.

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
    ("tests", "test tests spec specs e2e testing fixtures mocks mock cypress playwright example examples conftest factories "
              "demo demos sample samples benchmark benchmarks bench"),
    ("config", "config configs conf settings setup env deploy deployment infra infrastructure terraform k8s "
               "kubernetes helm docker ci scripts script build webpack vite rollup babel eslint jest tailwind "
               "postcss gulpfile gruntfile packaging release releases install installer homebrew formula doc docs documentation "
               "lint format typecheck generate codegen"),
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
# deep in a source tree these name a package (org/acme/samples/shop), not a folder of examples
WEAK_TESTS = set("example examples demo demos sample samples benchmark benchmarks bench fixtures".split())
TOKEN_LAYER: dict = {}
for _layer, _words in TOKENS:
    for _w in _words.split():
        TOKEN_LAYER.setdefault(_w, _layer)

# well-known libraries -> what kind of thing they are (used for layer hints and the "Built with" list)
EXT_CATS = {
    "web": ("Web framework", "flask fastapi django starlette aiohttp tornado sanic bottle werkzeug gunicorn uvicorn express koa "
            "rails sinatra hanami puma laravel/framework laravel/lumen-framework symfony/framework-bundle slim/slim "
            "spring-boot-starter-web spring-boot-starter-webflux ktor-server-core "
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

ENTRY_STEMS = {"main", "__main__", "manage", "wsgi", "asgi", "server", "app", "index", "cli", "run", "start", "program"}
# files that write down how the project is started (read by read_launch_files)
LAUNCH_FILES = [
    (re.compile(r"(?:.+\.)?Dockerfile(?:\..+)?$|.+\.dockerfile$|.+\.docker$", re.I), "docker"),
    (re.compile(r"Procfile(?:\..+)?$"), "procfile"),
    (re.compile(r".+\.ya?ml$"), "yaml"),            # compose, k8s, helm, CI, serverless
    (re.compile(r".+\.json$"), "json"),             # package / composer / deno scripts
    (re.compile(r".+\.toml$"), "toml"),             # [project.scripts] and friends
    (re.compile(r".+\.(?:service|conf|ini)$"), "ini"),   # systemd, supervisor, nginx / Apache
    (re.compile(r"(?:GNU)?[Mm]akefile$"), "make"),   # last: a Makefile mostly wraps the commands above
]
# files that list a project's dependencies: a tool named there is the project's own framework (next dev, rails s)
DEP_FILES = re.compile(r"package\.json$|composer\.json$|Gemfile$|pyproject\.toml$|requirements[\w.-]*\.txt$|Pipfile$|"
                       r"setup\.(?:py|cfg)$|go\.mod$|Cargo\.toml$|pom\.xml$|build\.gradle(?:\.kts)?$|.+\.csproj$|"
                       r"mix\.exs$|pubspec\.yaml$|deno\.json$")
CHDIR = re.compile(r"\b(?:chdir|Chdir|cd)\s*\(?\s*[\"']?(/?[\w./-]+)")

ROUTE_HINT = re.compile(
    r"@(?:\w+\.)?(?:route|get|post|put|delete|patch)\(|\b(?:app|router)\.(?:get|post|put|delete|patch|use)\(|"
    r"@RestController|@Controller|@(?:Get|Post|Put|Delete|Patch|Request)Mapping|HandleFunc\(|urlpatterns"
)

# comments and docstrings mention files and calls the code never makes; long strings hold messages, patterns, SQL
COMMENTS = re.compile(r"^[ \t]*[rRbBuU]{0,2}(\"\"\"|''')[\s\S]*?\1|/\*[\s\S]*?\*/|(?:^|(?<=\s))(?:#|//)(?=\s)[^\n]*", re.M)
STRINGS = re.compile(r"(\"\"\"|''')[\s\S]*?\1|\"(?:[^\"\\\n]|\\.){4,}\"|'(?:[^'\\\n]|\\.){4,}'")
# a file path written anywhere in a file: "viewer.html", ./lib.sh, templates/page.html, __DIR__."/../routes/web.php"
PATH_REF = re.compile(r"(?<![\w/.-])(/?(?:\.{1,2}/)*[\w-][\w.-]*(?:/[\w.-]+)*\.[A-Za-z]\w{0,7})(?![\w/-])")
STRING = re.compile(r"\"((?:[^\"\\\n]|\\.)+)\"|'((?:[^'\\\n]|\\.)+)'|`([^`\n]+)`")
PLACEHOLDER = re.compile(r"%[-+\d.]*[sdvif]|\$?\{[^{}]*\}|\*+")   # html/ex%d.html, pages/{name}.html, ${dir}/a.js
MODULE = re.compile(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+")          # food.settings, com.acme.App
STEM = re.compile(r"(?:/(?:__init__|index|mod))?\.\w+$")         # what a module name leaves out of its file path
# names too common to say which file is meant
COMMON_NAMES = set("main init new New run Run get set test setup start stop close Close String Error error handle "
                   "handler index update create delete render apply call parse load save build".split())

# where data comes in and goes out: key, plain-English label, direction, pattern in the source (any language)
IO = [
    ("args", "Command-line arguments", "in",
     r"\bsys\.argv\b|\bargparse\b|\bprocess\.argv\b|\bos\.Args\b|\bflag\.Parse\(|\bclap::|\bcobra\.Command\b|"
     r"\bcommander\b|\byargs\b|\bARGV\b|\$argv\b|CommandLine\.arguments|\bString\[\]\s*args\b|\bgetopts?\b|\"\$[@1]\""),
    ("env", "Environment variables", "in",
     r"\bos\.environ\b|\b[Gg]etenv\b|\bprocess\.env\b|\benv::var\b|\bENV\[|GetEnvironmentVariable\(|\bdotenv\b|"
     r"\$\{[A-Z][A-Z0-9_]*:?[-=]"),
    ("stdin", "Keyboard input", "in",
     r"(?<![\w.>:])input\(|\bsys\.stdin\b|\bprocess\.stdin\b|\bos\.Stdin\b|\bSystem\.in\b|\bscanf\(|\bConsole\.ReadLine\b|"
     r"\breadLine\(|^\s*read\s+-r\b|\$stdin\b|\bSTDIN\b"),
    ("http_in", "Web requests", "in",
     r"\.listen\(|ListenAndServe|\bapp\.run\(|\buvicorn\.run\(|\bserve_forever\(|\bcreateServer\(|SpringApplication\.run|"
     r"\bHttpServer\b|\baxum::serve\b|WebApplication\.Create"),
    ("file_in", "Files", "in",
     r"(?<![\w.>:])open\(|\bread_text\(|\bread_bytes\(|\breadFile(?:Sync)?\(|\bReadFile\(|\bos\.Open\(|\bFile\.read|"
     r"\bfs::read|\bfopen\(|\bFiles\.read|\bFile\.ReadAll|\bos\.walk\(|\breaddir(?:Sync)?\(|\bReadDir\(|\bget_data\("),
    ("http_resp", "Web responses", "out", None),   # whatever serves web requests answers them
    ("stdout", "Terminal output", "out",
     r"(?<![\w.>:])print\(|\bfmt\.(?:F?Print|Fprint)|\bprintln!|\bSystem\.out\.print|(?<![\w.>:])(?:printf|puts)\(|"
     r"\bConsole\.Write|\bprocess\.stdout\b|\bsys\.stdout\b|^\s*(?:puts|echo|printf)\s"),
    ("file_out", "Files", "out",
     r"\bwrite_text\(|\bwrite_bytes\(|\bwriteFile(?:Sync)?\(|\bWriteFile\(|\bos\.Create\(|\bFile\.write|\bfs::write|"
     r"\bFiles\.write|\bFile\.WriteAll|(?<![\w.>:])open\([^)\n]*['\"][wax]b?\+?['\"]|\bfopen\([^)\n]*\"[wa]|\bcreateWriteStream\("),
    ("http_out", "Web service calls", "out",
     r"\brequests\.(?:get|post|put|delete|patch|request)\(|\bhttpx\.|\burlopen\(|(?<![\w.>:])fetch\(|\baxios\b|"
     r"\bhttp\.(?:Get|Post|NewRequest)\(|\breqwest::|\bHttpClient\b|\bOkHttpClient\b|\bURLSession\b|\bNet::HTTP\b|"
     r"^\s*(?:curl|wget)\s"),
    ("proc", "Commands to other programs", "out",
     r"\bsubprocess\.|\bos\.system\(|\bexec\.Command\(|\bchild_process\b|\bexecSync\(|\bspawn\(|ProcessBuilder|"
     r"Runtime\.getRuntime\(\)\.exec|std::process::Command|Process\.Start\(|(?<![\w.>:])system\(|^\s*system\s|"
     r"\bos\.exec\w*\(|\bsyscall\.Exec\(|^\s*exec\s"),
    ("db", "Database", "both",
     r"\b(?:SELECT\s+[\w*,. ]+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+\s+SET|CREATE\s+TABLE)\b|\bsql\.Open\(|"
     r"\.connect\(\s*['\"](?:postgres|mysql|mongodb|redis|sqlite)"),
    ("queue", "Message queues", "both", None),   # known only from the libraries a file imports
]
IO_RES = [(k, re.compile(rx, re.M)) for k, _, _, rx in IO if rx]
IN_STRINGS = {"db", "env"}                       # SQL, connection URLs and "${HOME:-...}" live inside strings
IO_CAT = {"db": "db", "queue": "queue", "http": "http_out", "cloud": "http_out"}   # library kind -> I/O


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

def shebang(path: str):
    """The language of a script with no extension (artisan, bin/www, manage), from its #! line."""
    try:
        with open(path, "rb") as fh:
            line = fh.readline(200).decode("utf-8", "ignore")
    except OSError:
        return None
    m = re.match(r"#!\s*(\S+)(?:\s+(?:-\S+\s+)*(\S+))?", line)
    if not m:
        return None
    exe = posixpath.basename(m.group(1))
    exe = m.group(2) if exe == "env" and m.group(2) else exe   # #!/usr/bin/env php
    return languages.BY_INTERPRETER.get(re.sub(r"[\d.]+$", "", exe)) or languages.BY_INTERPRETER.get(exe)

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
            if any(rx.match(fn) for rx, _ in LAUNCH_FILES) and os.path.getsize(p) <= MAX_BYTES:
                man["RUN"].append(rel)
            if fn.lower().startswith("readme"):
                man["README"].append(rel)
            if DEP_FILES.match(fn):
                man["DEPS"].append(rel)
            plugin = languages.BY_EXT.get(os.path.splitext(fn)[1].lower()) or (None if "." in fn else shebang(p))
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


# ════════════════════════════════ 3. links any language has: paths and names ════════════════════════════════

def suffix_index(keys):
    """'a/b/c.py' is findable as 'c.py', 'b/c.py' and 'a/b/c.py'."""
    idx = defaultdict(list)
    for k in keys:
        parts = k.split("/")
        for i in range(len(parts)):
            idx["/".join(parts[i:])].append(k)
    return idx


def repo_path(d: str, rel: str) -> str:
    p = posixpath.normpath(posixpath.join(d, rel))
    return "" if p == "." else p


class Finder:
    """Repo files by path (templates/page.html) or by module name (food.settings, com.acme.App)."""

    def __init__(self, F):
        self.files, self.paths = sorted(F), suffix_index(F)
        stems = defaultdict(list)   # a/b.py, a/b/__init__.py, a/b/index.js, a/b/mod.rs -> a/b
        for p in F:
            for s in {STEM.sub("", p), re.sub(r"(?:\.\w+)+$", "", p)}:   # and welcome.blade.php -> welcome
                stems[s].append(p)
        self.stem = {s: ps[0] for s, ps in stems.items() if len(ps) == 1}
        self.stems = suffix_index(self.stem)

    def path(self, tok: str, d: str):
        h = find_path(tok, d, self.paths)
        if not h and "/" in tok and not re.search(r"\.\w+$", tok):   # '../config/application' -> config/application.rb
            h = find_path(tok, d, self.stems)
            h = self.stem.get(h) if h else None
        return h

    def module(self, name: str, d: str, bare=False):
        """'a.b.C' -> the file of module a/b/C, else a/b ('order.Order' is a model name, not the order package).
        bare: d is the folder a command runs in, and only a module right there counts (celery -A proj)."""
        parts = name.split(".")
        for k in range(len(parts), 0 if bare else 1, -1):
            m = "/".join(parts[:k])
            if bare:
                h = next((s for s in (posixpath.join(d, m, "__main__"), posixpath.join(d, m)) if s in self.stem), None)
            else:
                h = find_path(m + "/__main__", d, self.stems) or find_path(m, d, self.stems)
            if h:
                return self.stem[h]
        return None

    def pattern(self, s: str, d: str):
        """A file name built at run time ('html/ex%d.html', f"pages/{name}.html", 'sql/*.sql') -> the files it can be."""
        pieces = PLACEHOLDER.split(re.sub(r"^(?:\./|/)+", "", s))
        if len(pieces) < 2 or not re.search(r"\.[A-Za-z]\w{0,7}$", s) or not re.search(r"/|\w\w", pieces[0] + pieces[-1].rsplit(".", 1)[0]):
            return []   # '%s.py' alone could be any file
        rx = re.compile(r"(?:^|/)" + "[^/]+".join(re.escape(x) for x in pieces) + "$")
        hits = [p for p in self.files if rx.search(p)]
        if hits:   # the same pattern in several folders: keep the ones nearest the file that built it
            near = {h: len(os.path.commonprefix([d.split("/"), h.split("/")])) for h in hits}
            hits = [h for h in hits if near[h] == max(near.values())]
        return hits[:20]

    def command(self, cmd: str, d: str, bare=False, where=None):
        """Repo files a command line names: a path (node src/app.js, /app/entrypoint) or a module (python -m a.b,
        gunicorn a.wsgi:app). bare: d is the folder the command runs in, so a plain word is looked up there too.
        where: a path inside a container -> the repo path that was copied there."""
        out = []
        for w in re.findall(r"[^\s\"'`\[\](){},;|&<>=]+", re.split(r"\s\d?>>?\s*", cmd)[0]):
            if w.startswith(("-", "$")):
                continue
            here = None
            if where and w.startswith("/"):
                here = where(w)   # a path inside the container: the exact repo file copied there
                w = here or w
            mod = w.split(":")[0]
            exact = here if here is not None else repo_path(d, w)
            h = ((exact if exact in self.paths.get(exact, ()) else None)   # artisan, bin/www: a file named exactly
                 or (self.path(w, d) if re.search(r"[/.]", w) else None)
                 or (self.module(mod, d) if MODULE.fullmatch(mod) else None)
                 or (self.module(w, d, bare=True) if bare and re.fullmatch(r"[A-Za-z_]\w*", w) else None))
            if h:
                out.append(h)
        return out


def find_path(tok: str, d: str, idx):
    """A path written in a file inside folder d -> the repo file it names (None when unknown or ambiguous)."""
    rel = posixpath.normpath(posixpath.join(d, tok))
    if rel in idx.get(rel, ()):
        return rel
    s = re.sub(r"^(?:\.\./|/)+", "", posixpath.normpath(tok))
    if tok.startswith("/") and "/" not in s:   # "/__main__.py" is a piece of a path being built, not a path
        return None
    hits = idx.get(s, [])
    while not hits and tok.startswith("/") and "/" in s:   # /app/server.py inside a container -> server.py
        s = s.split("/", 1)[1]
        hits = idx.get(s, [])
    if len(hits) > 1:   # several files share that name: keep the one nearest the file that wrote it
        near = {h: len(os.path.commonprefix([d.split("/"), h.split("/")])) for h in hits}
        hits = [h for h in hits if near[h] == max(near.values())]
    return hits[0] if len(hits) == 1 else None


def link(F, plugins, find):
    """File -> files it names by path (templates, assets, scripts) or by module name in a string ("food.settings",
    "gunicorn app.wsgi"), plus files defining names it uses where the language needs no import for them."""
    defs = defaultdict(set)   # (language, scope, name) -> files that define it
    for p, f in F.items():
        imp = plugins[f["lang"]].implicit
        if imp == "none":
            continue
        scope = posixpath.dirname(p) if imp == "folder" else ""
        for s in f["symbols"]:
            if s["k"] != "route" and len(s["n"]) >= 3 and not s["n"].startswith("_") and s["n"] not in COMMON_NAMES:
                defs[(f["lang"], scope, s["n"])].add(p)
    refs = defaultdict(set)
    for p, f in F.items():
        d = posixpath.dirname(p)
        for tok in set(PATH_REF.findall(f["bare"])):
            refs[p].add(find.path(tok, d))
        strs = {a or b or c for a, b, c in STRING.findall(f["bare"])}
        for s in strs:   # frameworks wire code together with names in strings
            refs[p].update(find.command(s, d))
            refs[p].update(find.pattern(s, d))
        imp = plugins[f["lang"]].implicit
        if imp != "none":
            scope = d if imp == "folder" else ""
            words = set(re.findall(r"[A-Za-z_]\w*", f["code"]))
            words.update(w for s in strs for w in re.findall(r"\b[A-Z][a-z0-9]+[A-Z]\w*", s))   # 'AuthController@login'
            for w in words:
                hit = defs.get((f["lang"], scope, w))
                if hit and len(hit) == 1:   # a name two files define says nothing
                    refs[p] |= hit
        refs[p] -= {None, p}
    return refs


# ════════════════════════════════ 3b. how the project is started ════════════════════════════════

def read_launch_files(project):
    """Commands written down in launch files -> ([dict(name, cmd, file, cwd, serves, rank, parent)], where).
    cwd: the folder it runs in, a repo path, a path inside a container (starts with "/"), or None when unknown.
    serves: page files a web server config hands requests to. parent: the image a config was copied into.
    where(path) turns a container path into the repo path a Dockerfile COPY or a compose volume put there."""
    copies, raw, baked, conf_of = [], [], defaultdict(list), defaultdict(list)
    services = {}   # (folder, compose service) -> (working folder, image it is built from)
    exists = lambda rel: os.path.exists(os.path.join(project.root, rel))
    rank = {k: i for i, (_, k) in enumerate(LAUNCH_FILES)}

    def add(name, cmd, rel, cwd, kind, serves=""):
        raw.append(dict(name=name, cmd=cmd.strip(), file=rel, cwd=cwd, serves=serves, rank=rank[kind], parent=None))
        return raw[-1]

    for rel in project.man.get("RUN", []):
        text, d, fn = project.read(rel), posixpath.dirname(rel), posixpath.basename(rel)
        kind = next(k for rx, k in LAUNCH_FILES if rx.match(fn))
        if kind == "docker":
            wd, steps = "/", re.findall(r"^\s*(WORKDIR|COPY|ADD|CMD|ENTRYPOINT)\s+(.+?)\s*$", text, re.M | re.I)
            srcs_all = [w for kw, arg in steps if kw.upper() in ("COPY", "ADD") and "--from" not in arg
                        for w in arg.split()[:-1] if not w.startswith("--")]
            if any(not exists(repo_path(d, s)) and exists(repo_path("", s)) for s in srcs_all):
                d = ""   # built from the repo root (docker build -f sub/Dockerfile .), not from its own folder
            name = (re.sub(r"(?i)^dockerfile\.?|\.?(?:docker|dockerfile)$", "", fn)
                    or posixpath.basename(posixpath.dirname(rel)) or "container")
            run, copied = {}, []
            for kw, arg in steps:
                kw = kw.upper()
                if kw == "WORKDIR":
                    wd = posixpath.normpath(posixpath.join(wd, arg))
                elif kw in ("CMD", "ENTRYPOINT"):
                    run[kw] = (arg, wd)   # the last of each counts; CMD becomes the ENTRYPOINT's arguments
                elif "--from" not in arg:
                    *srcs, dst = [w for w in arg.split() if not w.startswith("--")]
                    into = len(srcs) > 1 or dst.endswith("/")
                    dst = posixpath.join(wd, dst)
                    for s in srcs:
                        src = repo_path(d, s)
                        copied.append(src)
                        folder = src == "" or src in project.dirs
                        copies.append((posixpath.normpath(posixpath.join(dst, posixpath.basename(s)) if into and not folder else dst), src))
            mine = []
            if run:
                cmd = " ".join(run[k][0] for k in ("ENTRYPOINT", "CMD") if k in run)
                mine = [add(name, cmd, rel, run.get("ENTRYPOINT", run.get("CMD"))[1], kind)]
                mine[0]["copied"] = copied
            for src in copied:   # configs baked into the image: its server pages and supervised programs
                baked[src] += mine
        elif kind == "yaml":   # compose services, k8s containers, CI jobs: `command:` and friends, named by their block
            lines = text.splitlines()

            def block(j):   # the lines of the block that opens at line j, with any `<<: *anchor` it merges in
                top, out = len(lines[j]) - len(lines[j].lstrip()), []
                for b_ in lines[j + 1:]:
                    if b_.strip() and len(b_) - len(b_.lstrip()) <= top:
                        break
                    out.append(b_)
                    m_ = re.match(r"^\s*<<\s*:\s*\*([\w.-]+)", b_)
                    a_ = m_ and next((x for x, l_ in enumerate(lines) if re.search(r"&" + re.escape(m_.group(1)) + r"\b", l_)), None)
                    if a_ is not None and a_ != j:
                        out += block(a_)
                return out

            def image_of(blk):   # built from this repo: the Dockerfile it is built from
                ctx = next((m_ for m_ in (re.match(r"^\s*(?:context|build)\s*:\s*[\"']?([^\s\"'{]+)", b_) for b_ in blk) if m_), None)
                df = next((m_ for m_ in (re.match(r"^\s*dockerfile\s*:\s*[\"']?([^\s\"']+)", b_) for b_ in blk) if m_), None)
                if not ctx:
                    return None
                c_ = repo_path(d, ctx.group(1))
                return repo_path(c_, df.group(1)) if df else repo_path(c_, "Dockerfile")

            def folder_of(blk):   # its working_dir (else the image's own WORKDIR, see find_launches)
                return next((m_.group(1) for m_ in (re.match(r"^\s*working_dir\s*:\s*[\"']?([^\s\"']+)", b_)
                                                    for b_ in blk) if m_), None)
            for i, line in enumerate(lines):
                if re.match(r"^\s*(?:build|context|working_dir)\s*:", line):   # remember how each service is built
                    j = next((j for j in range(i - 1, -1, -1) if lines[j].strip() and
                              len(lines[j]) - len(lines[j].lstrip()) < len(line) - len(line.lstrip()) and
                              re.match(r"^\s*[\w.-]+\s*:\s*$", lines[j]) and lines[j].strip()[:-1] not in ("build",)), None)
                    if j is not None:
                        key = (d, lines[j].strip()[:-1].strip())
                        if key not in services:
                            services[key] = (folder_of(block(j)), image_of(block(j)))
                vol = re.match(r"^\s*-\s*[\"']?(\.{1,2}(?:/[^:\"']*)?)\s*:\s*(/[^:\"'\s]*)", line)   # - ./code:/app/code
                if vol:
                    copies.append((posixpath.normpath(vol.group(2)), repo_path(d, vol.group(1))))
                m = re.match(r"^(\s*)(?:-\s+)?(command|entrypoint|cmd|run|start|script|exec)\s*:\s*(.*)$", line)
                if not m:
                    continue
                ind, val = len(m.group(1)), m.group(3).strip()
                if val in ("", "|", ">", "|-", ">-"):   # the command continues on the lines below
                    vals = []
                    for nxt in lines[i + 1:]:
                        k = len(nxt) - len(nxt.lstrip())
                        if nxt.strip() and (k < ind or (k == ind and not nxt.lstrip().startswith("- "))):
                            break
                        vals.append(re.sub(r"^\s*-\s*", "", nxt).strip())
                    val = " ".join(v for v in vals if v)
                name, wd, image = fn.rsplit(".", 1)[0], None, None
                for j in range(i - 1, -1, -1):   # the block it belongs to: a compose service, a container, a job
                    k = len(lines[j]) - len(lines[j].lstrip())
                    pm = re.match(r"^\s*(?:-\s+)?(?:name\s*:\s*)?[\"']?([\w.-]+)[\"']?\s*:?\s*$", lines[j])
                    if lines[j].strip() and k < ind and pm:
                        name, blk = pm.group(1), block(j)
                        wd, image = folder_of(blk), image_of(blk)
                        break
                add(name, val, rel, wd, kind)["image"] = image
        elif kind == "procfile":
            for n, c in re.findall(r"^([\w-]+)\s*:\s*(.+)$", text, re.M):
                add(n, c, rel, d, kind)
        elif kind == "make":
            var = dict(re.findall(r"^(\w+)\s*[:?+]?=\s*(.*)$", text, re.M))
            sub = lambda s: re.sub(r"\$[({](\w+)[)}]", lambda v: var.get(v.group(1), "$" + v.group(1)), s)
            for n, blk in re.findall(r"^([\w.-]+)\s*:(?!=)[^\n]*\n((?:\t[^\n]*(?:\n|$))+)", text, re.M):
                for c in blk.splitlines():
                    add(n, sub(sub(c)).strip().lstrip("@-+ "), rel, d, kind)
        elif kind == "ini":   # supervisor [program:x] command=, systemd ExecStart=; nginx root, Apache DocumentRoot
            sec = fn.rsplit(".", 1)[0]
            for line in text.splitlines():
                h = re.match(r"^\s*\[(?:\w+:)?([^\]]+)\]", line)
                sec = h.group(1).strip() if h else sec
                c = re.match(r"^\s*(?:command|ExecStart)\s*=\s*[-@+!:]*(.+)$", line)
                if c:
                    conf_of[rel].append(add(sec, c.group(1), rel, None, kind))
            for root_ in re.findall(r"^\s*(?:root|DocumentRoot)\s+[\"']?([^\s;\"']+)", text, re.M | re.I):
                idx = re.findall(r"^\s*(?:index|DirectoryIndex)\s+([^;\n]+)", text, re.M | re.I)
                pages = dict.fromkeys((idx[0].split() if idx else []) + ["index.php", "index.html", "index.js"])
                conf_of[rel].append(add(fn.rsplit(".", 1)[0], "", rel, None, kind,
                                        serves=" ".join(posixpath.join(root_, x) for x in pages)))
        elif kind == "json":   # package.json / composer.json / deno.json: "scripts", "tasks", "bin"
            try:
                j = json.loads(text)
            except ValueError:
                continue
            for key in ("scripts", "tasks", "bin") if isinstance(j, dict) else ():
                v = j.get(key)
                for n, c in ({j.get("name") or key: v} if isinstance(v, str) else v if isinstance(v, dict) else {}).items():
                    if not re.match(r"(?:pre|post)[A-Z_:-]|(?:pre|post)(?:install|publish|pack|version|test)", n):   # hooks
                        add(n, " ".join(map(str, c)) if isinstance(c, list) else str(c), rel, d, kind)
        elif kind == "toml":   # [project.scripts], [tool.poetry.scripts] ...
            for blk in re.findall(r"^\[[\w.-]*scripts\]\s*\n(.*?)(?=^\[|\Z)", text, re.M | re.S):
                for n, c in re.findall(r"^\s*[\"']?([\w.-]+)[\"']?\s*=\s*[\"']([^\"'\n]+)", blk, re.M):
                    add(n, c, rel, d, kind)

    for L in raw:   # compose.override.yml adds a command to a service compose.yml builds: same service, same folder
        base = services.get((posixpath.dirname(L["file"]), L["name"]))
        if L.get("image", 1) is None and base:
            L["cwd"] = L["cwd"] if L["cwd"] is not None else base[0]
            L["image"] = base[1]
    for conf, items in conf_of.items():   # a config copied into an image runs inside that image
        hosts = baked.get(conf, [])
        for L in items:
            if L["serves"] and hosts:   # its web root: the image's own command serves those pages
                for H in hosts:
                    H["serves"] = (H["serves"] + " " + L["serves"]).strip()
                raw.remove(L)
            elif hosts:
                L["parent"] = hosts[0]

    def where(path):
        path = posixpath.normpath(path)
        best = max((c for c in copies if path == c[0] or path.startswith(c[0].rstrip("/") + "/")),
                   key=lambda c: len(c[0]), default=None)
        if best:
            return repo_path(best[1], path[len(best[0]):].lstrip("/"))
        for dst, src in copies:   # a folder above what was copied: /app/api holds /app/api/src <- api/src, so it is api
            tail = dst[len(path):].strip("/") if dst.startswith(path.rstrip("/") + "/") else None
            if tail and (src == tail or src.endswith("/" + tail)):
                return src[: len(src) - len(tail)].rstrip("/")
        return None
    return raw, where


def dependencies(rel: str, text: str) -> list:
    """Names of what a project needs to run (not its dev tools), from its dependency file."""
    fn = posixpath.basename(rel)
    if fn.endswith(".json"):   # package.json, composer.json
        try:
            j = json.loads(text)
        except ValueError:
            return []
        return [k for sec in ("dependencies", "peerDependencies", "require")
                if isinstance(j, dict) and isinstance(j.get(sec), dict) for k in j[sec]]
    if fn == "go.mod":
        return re.findall(r"^\s*(?:require\s+)?([\w.-]+\.[\w.-]+/[\w./-]+)\s+v\d", text, re.M)
    if fn == "Gemfile":
        return re.findall(r"^\s*gem\s+['\"]([\w-]+)", text, re.M)
    if fn.endswith(".txt"):   # requirements.txt
        return re.findall(r"^\s*([A-Za-z][\w.-]*)", text, re.M)
    if fn.endswith((".toml", ".cfg", ".py")) or fn == "Pipfile":
        out = []
        for head, body in re.findall(r"(?ms)^\[([^\]]+)\]\s*$(.*?)(?=^\[|\Z)", text) or [("", text)]:
            if re.search(r"dev|test|lint|doc|group", head):   # tools for working on it, not for running it
                continue
            out += re.findall(r"[\"']([A-Za-z][\w.-]*)(?:\[[^\]]*\])?(?:[<>=~!;,@][^\"']*)?[\"']", body)   # "fastapi[std]>=1"
            if head.endswith(("dependencies", "packages")):   # poetry / Pipfile style: django = "^4"
                out += re.findall(r"^([A-Za-z][\w.-]*)\s*=", body, re.M)
        return out
    return [x for g in re.findall(r"<artifactId>([\w.-]+)<|['\"]([\w.-]+):[\w.-]+:|Include=\"([\w.-]+)\"", text)
            for x in g if x]   # Maven, Gradle, .csproj


def find_launches(project, F, find, deps):
    """Every way the project is started: a command in a launch file, or a command one of its own files runs
    (a container entrypoint picking `gunicorn app.wsgi` or `celery -A app worker`).
    -> [dict(name, cmd, file, targets, via, cwd, rank)], `via` being the launch that started this one."""
    raw, where = read_launch_files(project)
    for L in raw:   # a compose service runs in its image's WORKDIR unless it says otherwise
        D = L.get("image") and next((M for M in raw if M["file"] == L["image"]), None)
        if L["cwd"] is None and D:
            L["cwd"] = D["cwd"]
    for L in raw:
        if L["cwd"] is not None and L["cwd"].startswith("/"):
            L["cwd"] = where(L["cwd"])
        d = posixpath.dirname(L["file"]) if L["cwd"] is None else L["cwd"]
        L["pages"] = find.command(L["serves"], d, where=where)
        L["targets"] = list(dict.fromkeys(find.command(L["cmd"], d, bare=L["cwd"] is not None, where=where) + L["pages"]))
    mains = sorted(p for p, f in F.items() if f["entry"] and f["layer"] != "tests")

    def program(L):   # an image that runs something built from the code it copies in: that code's main()
        inside = [m for m in mains if any(not s or m == s or m.startswith(s + "/") for s in L.get("copied") or ())]
        top = min((m.count("/") for m in inside), default=0)
        return [m for m in inside if m.count("/") == top]

    words = lambda c: re.findall(r"[^\s\"'\[\],]+", c)
    built = {}   # launch -> the arguments it passes to a program built from this repo
    for L in raw:   # ENTRYPOINT ["/app"], CMD ["air"]: not a file of the repo, nor a tool it depends on, so the image
        w = words(L["cmd"])   # runs a program built from the code it copied in
        if not L["targets"] and L.get("copied") and w and not deps & {posixpath.basename(x).lower() for x in w[:2]}:
            L["targets"], built[id(L)] = program(L), w[1:]
    for L in raw:
        img = L.get("image")
        if not L["targets"] and img:   # a compose service of an image built here: `command: ["serve"]` are its arguments
            D = next((M for M in raw if M["file"] == img), None)
            if D and id(D) in built:
                L["targets"], built[id(L)] = list(D["targets"]), words(L["cmd"])
        if id(L) in built and L["targets"]:   # `serve`, `worker`: the one file of the program with that name
            home = posixpath.dirname(L["targets"][0])
            for w in (x for a in built[id(L)] for x in re.findall(r"[A-Za-z][\w-]+", a)):
                hits = [q for q in F if q.startswith(home) and posixpath.basename(q).rsplit(".", 1)[0] == w
                        and F[q]["lang"] == F[L["targets"][0]]["lang"]]
                if len(hits) == 1 and hits[0] not in L["targets"]:
                    L["targets"].append(hits[0])
    found = list(raw)
    proc = dict(IO_RES)["proc"]
    for p, f in F.items():   # commands a program builds and runs itself
        if f["layer"] == "tests" or not proc.search(f["code"]):
            continue
        cd = CHDIR.search(f["bare"])
        cwd = None if not cd else where(cd.group(1)) if cd.group(1).startswith("/") else repo_path(posixpath.dirname(p), cd.group(1))
        for m in STRING.finditer(f["bare"]):
            s = next(g for g in m.groups() if g is not None)
            if len(s.split()) < 2:
                continue
            hits = [h for h in find.command(s, posixpath.dirname(p) if cwd is None else cwd, bare=cwd is not None, where=where) if h != p]
            if hits:
                key = re.findall(r"[\"']([\w-]+)[\"']\s*:", f["bare"][max(0, m.start() - 300):m.start()])
                found.append(dict(name=key[-1] if key else s.split()[0], cmd=s, file=p, targets=list(dict.fromkeys(hits)),
                                  cwd=cwd, serves="", pages=[], rank=-1, parent=None))
    for L in found:   # a command started by another one: by an image it was baked into, or from a file it runs
        L["via"] = L.pop("parent") or next((P for P in found if P is not L and L["file"] in P["targets"]), None)
    found.sort(key=lambda L: L["rank"] if L["via"] is None else -1)   # Docker / compose / Procfile before Makefile
    return found


def conventions(F):
    """Files a framework loads by name or by scanning a folder, which no code imports:
    C1 a name repeated in sibling packages (apps.py, migrations/), C2 a folder of look-alike files (pages/, commands/).
    -> ({anchor file: {member files}}, [members with no anchor]). The anchor is the package file that stands for
    the package, so the members join any journey that reaches it."""
    live = {p for p, f in F.items() if f["layer"] != "tests"}
    users = Counter(q for p in live for q in F[p]["uses"])
    own, under = defaultdict(list), defaultdict(list)
    for p in sorted(live):
        own[posixpath.dirname(p)].append(p)
        d = posixpath.dirname(p)
        while True:
            under[d].append(p)
            if not d:
                break
            d = posixpath.dirname(d)
    free = lambda ps: not any(users[p] or F[p]["entry"] for p in ps)   # nothing imports or starts them
    # 0001_initial.py, 2017_10_13_create_users.php: ordered one-off steps (migrations), run by a command of their own
    numbered = lambda p: bool(re.match(r"[vV]?\d{3,}", posixpath.basename(p)))

    def anchor(d, skip=()):
        while d:
            cands = [p for p in own.get(d, ()) if p not in skip]
            if cands:
                return min(cands, key=lambda p: (posixpath.basename(p).rsplit(".", 1)[0] not in ("__init__", "index", "mod"),
                                                 -users[p], p))   # the package file, else the most used one
            d = posixpath.dirname(d)
        return None

    found, loose = defaultdict(set), set()

    def add(d, members):
        a = anchor(d, members)
        if a:
            found[a] |= set(members)
        else:
            loose.update(members)

    kids = defaultdict(list)   # folder -> its sub-folders that hold code
    for d in under:
        if d:
            kids[posixpath.dirname(d)].append(d)
    for _, subs in sorted(kids.items()):   # C1
        names = defaultdict(list)
        for s in subs:
            for p in own.get(s, ()):
                if posixpath.basename(p).rsplit(".", 1)[0] not in ("__init__", "index", "mod"):
                    names[posixpath.basename(p)].append((s, [p]))
            for c in kids.get(s, ()):
                names[posixpath.basename(c) + "/"].append((s, under[c]))
        for _, places in names.items():
            unused = [(s, [m for m in ms if not numbered(m)]) for s, ms in places if free(ms)]
            if len(places) >= 2 and len(unused) == len(places):   # imported nowhere: the framework finds it by name
                for s, ms in unused:
                    if ms:
                        add(s, ms)
    for d, ps in own.items():   # C2
        ext = Counter(posixpath.splitext(p)[1] for p in ps).most_common(1)
        unused = [p for p in ps if free([p]) and posixpath.splitext(p)[1] == ext[0][0]] if ext else []
        if (d and len(unused) >= 3 and 2 * len(unused) >= len(ps) and not all(numbered(p) for p in unused)
                and Counter(F[p]["layer"] for p in unused).most_common(1)[0][0] != "config"):
            add(d, [p for p in unused if not numbered(p)])
    return found, loose


def find_io(f) -> list:
    """What the file takes in and gives out, from its source text and the libraries it imports."""
    found = {k for k, rx in IO_RES if rx.search(f["bare"] if k in IN_STRINGS else f["code"])}
    found |= {IO_CAT[c] for c in (cat_of(spec) for _, spec in f["ext"]) if c in IO_CAT}
    if any(s["k"] == "route" for s in f["symbols"]):
        found.add("http_in")
    if "http_in" in found:
        found.add("http_resp")
    return [k for k, *_ in IO if k in found]


# ════════════════════════════════ 4. classify into layers ════════════════════════════════

def classify(f, fullstack: bool, plugin):
    parts = f["path"].split("/")
    segs = [parts[-1].rsplit(".", 1)[0]] + parts[-2::-1]  # file stem first, then folders deepest -> shallowest
    all_toks = [toks(s) for s in segs]
    depth = len(segs) - 1
    all_toks = [[t for t in ts if t not in WEAK_TESTS or i == 0 or depth - i < 2] for i, ts in enumerate(all_toks)]
    if any(TOKEN_LAYER.get(t) == "tests" for ts in all_toks for t in ts):
        return "tests"
    if any(t in ("config", "configs", "conf") for ts in all_toks[1:] for t in ts):   # config/database.php is a setting
        return "config"
    for i, ts in enumerate(all_toks):
        for t in reversed(ts):
            ly = TOKEN_LAYER.get(t)
            if ly:
                if t == "client":
                    return "interface" if fullstack else "integration"
                if t in plugin.stem_layers:
                    return plugin.stem_layers[t]
                return ly
    cats = {cat_of(spec) for _, spec in f["ext"]}
    for cat, layer in CAT_LAYER:
        if cat in cats:
            return layer
    hint = plugin.layer_hint(f)
    if hint:
        return hint
    # endpoints the language found; raw text only where the language has no exact route finder of its own
    if any(s["k"] == "route" for s in f["symbols"]) or (plugin.regex_routes and ROUTE_HINT.search(f["code"])):
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
        f["bare"] = COMMENTS.sub(" ", f["text"])                   # what the code says, not what it explains
        f["code"] = STRINGS.sub('""', f["bare"])
    project = Project(root, files, man)
    find = Finder(files)
    for name_ in sorted({f["lang"] for f in files.values()}):
        plugins[name_].prepare(project)                            # read go.mod, package.json ...
    refs = link(files, plugins, find)                              # paths and names, any language
    for p, f in files.items():
        uses, ext = plugins[f["lang"]].resolve(project, f)
        f["uses"], f["ext"] = sorted((uses | refs[p]) - {p}), sorted(ext)   # sorted: identical output on every run
    top_dirs = {p.split("/")[0].lower() for p in files if "/" in p}
    fullstack = bool(top_dirs & {"client", "frontend", "web"}) and bool(top_dirs & {"server", "backend", "api"})
    for f in files.values():
        f["layer"] = classify(f, fullstack, plugins[f["lang"]])
        f["io"] = find_io(f) if f["layer"] != "tests" else []
    F = files

    # ---- files the framework loads by convention, and the commands that start the program
    conv_found, conv_loose = conventions(F)
    conv = {(a, m) for a, ms in conv_found.items() for m in ms if m not in F[a]["uses"]}
    for a, m in sorted(conv):
        F[a]["uses"].append(m)
    conv_roots = sorted(conv_loose | {a for a in conv_found if not any(a in f["uses"] for f in F.values())})
    deps = {w.lower() for rel in man.get("DEPS", []) for w in dependencies(rel, project.read(rel))}
    launches = find_launches(project, F, find, deps)

    # ---- parts (units) and layer assignment
    units, tree, loose, sub, squash = build_units(F)
    layer_name = {k: n for k, n, *_ in LAYERS}
    unit_of, ulayer = {}, {}
    for u in units:
        c = Counter()
        for p in u["files"]:   # files with no clue count as logic, so one small named file can't decide a folder
            c[F[p]["layer"] or "logic"] += F[p]["loc"]
        ulayer[u["id"]] = u.get("layer") or c.most_common(1)[0][0]
        for p in u["files"]:
            F[p]["layer"] = F[p]["layer"] or ulayer[u["id"]]
            unit_of[p] = u["id"]

    # ---- processes: launch commands > declared / main() entry points > conventional file names > public API
    def reach(starts):
        seen, todo = set(starts), list(starts)
        for p in todo:
            for q in F[p]["uses"]:
                if q not in seen and F[q]["layer"] != "tests":
                    seen.add(q)
                    todo.append(q)
        return seen

    procs, entry_kind = [], "Entry points"
    for L in launches:
        if any(M["via"] is L for M in launches):
            continue   # it starts another command: part of the way in, not a process of its own
        chain, P = [L], L["via"]
        while P and P not in chain and len(chain) < 6:
            chain.insert(0, P)
            P = P["via"]
        said = toks(L["name"]) + [t for w in re.findall(r"[^\s\"'\[\],]+", L["cmd"]) if not re.search(r"[/.$-]", w)
                                  for t in toks(w)]
        if len(chain) == 1 and any(TOKEN_LAYER.get(t) in ("tests", "config") for t in said):
            continue   # test, lint, build, install, deploy ...
        starts = [s for s in L["targets"] if F[s]["layer"] != "tests"]
        tool = [posixpath.basename(w).lower() for w in re.findall(r"[^\s\"'\[\],=]+", L["cmd"])[:2] if not w.startswith("-")]
        if not starts and deps & set(tool):   # a tool the project depends on runs it from a folder (`next dev`):
            d = L["cwd"] if L["cwd"] is not None else posixpath.dirname(L["file"])   # it finds main.py / index.js there,
            inside = [q for q in F if (not d or q.startswith(d + "/")) and F[q]["layer"] != "tests"]   # or convention files
            users = {(f["lang"], u) for f in F.values() if f["layer"] != "tests" for u in f["uses"]}
            named = [q for q in inside if posixpath.basename(q).rsplit(".", 1)[0] in ENTRY_STEMS
                     and plugins[F[q]["lang"]].entry_by_name and (F[q]["lang"], q) not in users
                     and q[len(d):].strip("/").count("/") <= 2]
            top = min((q.count("/") for q in named), default=0)
            starts = [q for q in named if q.count("/") == top]
            near = {posixpath.dirname(q) for q in starts}   # pages/index.tsx and the look-alike pages beside it
            starts += [m for m in conv_roots if m in inside and (not starts or posixpath.dirname(m) in near)]
        key = sorted(set(starts))
        same = next((P for P in procs if P["key"] == key), None)
        if same:   # one program started several ways: keep the names given in the same kind of file
            names_ = same["name"].split(" / ")
            if L["name"] not in names_ and len(names_) < 3 and chain[0]["rank"] == same["chain"][0]["rank"]:
                same["name"] += " / " + L["name"]
        elif starts:
            procs.append(dict(name=L["name"], cmd=L["cmd"], chain=chain, starts=list(dict.fromkeys(starts)), key=key))
    declared = {h for h in project.declared if h in F}
    imported = {q for f in F.values() if f["layer"] not in ("tests", "config") for q in f["uses"]}
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
    pool = [(k, p) for k, p in cand if F[p]["layer"] != "config"] or cand
    reached = set().union(*(reach(P["starts"]) for P in procs)) | {s for P in procs for c in P["chain"] for s in [c["file"]] + c["targets"]}
    launched = bool(procs)
    for (pr, *_), p in pool:   # programs no launch command covers: one process each
        if p not in reached and not (launched and pr == 2):
            procs.append(dict(name=p, cmd="", chain=[], starts=[p], key=[p]))
            reached |= reach([p])
    for P in procs:   # a start that names nothing in the project boots it by scanning the folder below it
        d = posixpath.dirname(P["starts"][0])
        if d and not any(F[s]["uses"] for s in P["starts"]):
            users = {q for f in F.values() for q in f["uses"]}
            for q in sorted(F):
                if q.startswith(d + "/") and q not in users and F[q]["layer"] != "tests" and q not in P["starts"]:
                    F[P["starts"][0]]["uses"].append(q)
                    conv.add((P["starts"][0], q))
    lone = lambda P: (not P["cmd"] and len(reach(P["starts"]) - set(P["starts"])) == 0
                      and all(F[s]["text"].startswith("#!") for s in P["starts"]))
    if any(not lone(P) for P in procs):   # bin/bundle, mvnw: scripts wrapping a tool, not the program itself
        procs = [P for P in procs if not lone(P)]
    home = lambda q: max((posixpath.dirname(m) for m in man.get("DEPS", []) if q.startswith(posixpath.dirname(m) + "/")
                          or not posixpath.dirname(m)), key=len, default="")
    on_way = set().union(*(reach(P["starts"]) for P in procs)) if procs else set()
    for m in conv_loose - on_way:   # loaded by the framework of the project it sits in, for each of its programs
        for P in procs:
            if home(P["starts"][0]) == home(m):
                F[P["starts"][0]]["uses"].append(m)
                conv.add((P["starts"][0], m))
    if not procs:  # a library: its "front door" is the package's top-level index
        lib = [p for p in F if posixpath.basename(p).rsplit(".", 1)[0] in ("__init__", "index", "lib", "mod")
               and F[p]["layer"] != "tests" and p.count("/") <= 3]
        if lib:
            m = min(p.count("/") for p in lib)
            starts = sorted((p for p in lib if p.count("/") == m), key=lambda p: -len(F[p]["uses"]))[:3]
            procs, entry_kind = [dict(name="Public API", cmd="", chain=[], starts=starts, key=starts)], "Public API"
    merged = []
    for P in procs:   # bin/rails and bin/rake lead to the same program
        same = next((M for M in merged if reach(M["starts"]) == reach(P["starts"])
                     and not M["cmd"] and not P["cmd"]
                     and posixpath.dirname(M["starts"][0]) == posixpath.dirname(P["starts"][0])), None)
        if same:
            if same["name"].count(" / ") < 2:
                first = same["name"] if " / " in same["name"] else posixpath.basename(same["name"])
                same["name"] = first + " / " + posixpath.basename(P["name"])
            same["starts"] += [s for s in P["starts"] if s not in same["starts"]]
        else:
            merged.append(P)
    procs = merged[:8]
    names_ = Counter(P["name"] for P in procs)
    for P in procs:   # two `backend` services: say which file each one comes from
        if names_[P["name"]] > 1 and P["chain"]:
            P["name"] += f" ({posixpath.basename(P['chain'][0]['file'])})"
    base = Counter(posixpath.basename(P["name"]) for P in procs)
    for P in procs:   # lesson1/main.go, lesson2/main.go ... keep their folders; a lone app/main.py is just main.py
        if not P["cmd"] and " / " not in P["name"] and base[posixpath.basename(P["name"])] == 1:
            P["name"] = posixpath.basename(P["name"])
    entries = list(dict.fromkeys(s for P in procs for s in P["starts"]))
    entry_set = set(entries) | {c["file"] for P in procs for c in P["chain"] if c["file"] in F}

    # ---- dependency edges between files
    fedges = Counter((p, q) for p, f in F.items() for q in f["uses"] if q in F)

    # ---- node tree: layer > part > folder > file
    nodes = {}

    def mk(id_, name_, kind, path, fl, parent):
        langs = Counter()
        for p in fl:
            langs[F[p]["lang"]] += F[p]["loc"]
        io = {k for p in fl for k in F[p]["io"]}
        n = dict(id=id_, name=name_, kind=kind, path=path, files=len(fl), loc=sum(F[p]["loc"] for p in fl),
                 langs=[[l, c] for l, c in langs.most_common(4)], children=[], edges=[], uses=[], usedBy=[],
                 parent=parent, layer=None, desc="", io=[k for k, *_ in IO if k in io], _files=fl)
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

    # ---- dependency edges between siblings (drawn as lines in the GUI; a 4th item 1 = only convention links)
    edge = lambda a, b, w, plain: [a, b, w] + ([] if plain[(a, b)] else [1])
    for n in list(nodes.values()):
        if not n["children"]:
            continue
        owner = {p: c for c in n["children"] for p in nodes[c]["_files"]}
        cnt, plain = Counter(), Counter()
        for p in n["_files"]:
            for q in F[p]["uses"]:
                a, b = owner.get(p), owner.get(q)
                if a and b and a != b:
                    cnt[(a, b)] += 1
                    plain[(a, b)] += (p, q) not in conv
        n["edges"] = [edge(a, b, w, plain) for (a, b), w in cnt.most_common(80)]

    uedges, uplain = Counter(), Counter()
    for (p, q), w in fedges.items():
        if unit_of[p] != unit_of[q]:
            uedges[(unit_of[p], unit_of[q])] += 1
            uplain[(unit_of[p], unit_of[q])] += (p, q) not in conv
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
                  edges=[edge(a, b, w, uplain) for (a, b), w in uedges.most_common() if a in inside and b in inside][:80])
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

    # ---- flow: for each process, which parts does it touch, in what order?
    flow = None
    label = {k: lab for k, lab, *_ in IO}
    both = lambda xs: ", ".join(xs[:-1]) + " and " + xs[-1] if len(xs) > 1 else xs[0]

    def short(cmd):   # 'gunicorn app.wsgi --bind 0.0.0.0' -> 'gunicorn app.wsgi'; 'celery -A app worker -l INFO' -> 'celery -A app worker'
        words = []
        for w in re.findall(r"[^\s\"'\[\],]+", cmd):
            if w.startswith("--") or len(words) == 4 or (w.startswith("-") and len(words) >= 3):
                break
            if not w.startswith("$"):
                words.append(w)
        return " ".join(words)

    def names(st):   # three main.go files read better as lesson1/main.go, lesson2/main.go ...
        nm = [nodes[u]["name"] for u in st["units"][:3]]
        return ", ".join(nodes[u]["path"] if nm.count(n) > 1 and nodes[u]["path"] else n for u, n in zip(st["units"], nm))

    if procs:
        adj = defaultdict(dict)
        for (a, b), w in uedges.items():
            adj[a][b] = w
        skip = {u for u in ulayer if ulayer[u] == "tests"}
        out, seen = [], set()
        for P in procs:
            start = list(dict.fromkeys(unit_of[s] for s in P["starts"]))
            dist, queue = {u: 0 for u in start}, list(start)
            for u in queue:
                for v in adj[u]:
                    if v not in dist and v not in skip:
                        dist[v] = dist[u] + 1
                        queue.append(v)
            seen |= set(dist) | {unit_of[s] for c in P["chain"] for s in [c["file"]] + c["targets"] if s in F}
            by_stage = defaultdict(list)
            for u, d in dist.items():
                by_stage[min(d, 4)].append(u)
            stages = []
            for s in sorted(by_stage):
                prev = stages[-1]["all"] if stages else []
                us = sorted(by_stage[s], key=lambda u: (-sum(adj[p].get(u, 0) for p in prev), -nodes[u]["loc"], u))
                stages.append(dict(units=us[:7], more=max(0, len(us) - 7), all=us))
            shown = {u: i for i, st in enumerate(stages) for u in st["units"]}
            edges = [edge(a, b, w, uplain) for (a, b), w in uedges.items() if a in shown and b in shown and shown[b] > shown[a]]
            text = ""
            if P["chain"]:   # how it is started: Dockerfile runs entrypoint.py, which runs `gunicorn app.wsgi`
                steps = [c["targets"][0] for c in P["chain"][:-1] if c["targets"]]
                if P["chain"][-1]["cmd"]:
                    steps.append("`" + short(P["chain"][-1]["cmd"]) + "`")
                text = posixpath.basename(P["chain"][0]["file"]) + (" runs " + ", which runs ".join(steps) if steps else "")
                if P["chain"][-1]["pages"]:
                    text += (", and" if steps else "") + " sends web requests to " + ", ".join(P["chain"][-1]["pages"][:2])
                text += ". "
            text += ("It starts in " if entry_kind == "Entry points" else "People using this library start from ") + names(stages[0])
            if len(stages) > 1:
                text += f", which calls into {names(stages[1])}"
            if len(stages) > 2:
                text += f", and then relies on {names(stages[2])}"
            text += "."
            io_units = defaultdict(list)   # inputs and outputs of every part it reaches, nearest the start first
            for u in sorted(dist, key=lambda u: (dist[u], -nodes[u]["loc"], u)):
                for k in nodes[u]["io"]:
                    io_units[k].append(u)
            words = lambda way: list(dict.fromkeys(label[k].lower() for k, _, w, _ in IO if io_units[k] and w in (way, "both")))
            ins, outs = words("in"), words("out")
            if ins:
                text += f" It takes in {both(ins)}"
            if outs:
                text += (", and gives out" if ins else " It gives out") + f" {both(outs)}"
            if ins or outs:
                text += "."
            out.append(dict(name=P["name"], command=short(P["cmd"]), narrative=text, entries=P["starts"],
                            chain=[c["file"] for c in P["chain"] if c["file"] in F],
                            stages=[dict(units=s["units"], more=s["more"]) for s in stages], edges=edges,
                            io={k: io_units[k][:4] for k, *_ in IO if io_units[k]}))
        unreached = sorted((u for u in ulayer if u not in seen and ulayer[u] != "tests"),
                           key=lambda u: (ulayer[u] == "config", -nodes[u]["loc"], u))
        flow = dict(kind=entry_kind, processes=out, unreached=unreached)

    # ---- repository summary
    ext_files, ext_spec = defaultdict(set), {}
    for p, f in F.items():
        for nm, spec in f["ext"]:
            ext_files[nm].add(p)
            ext_spec.setdefault(nm, spec)
    for nm in sorted(deps):   # what the dependency files name (gems and composer packages are rarely imported by name)
        if cat_of(nm) and nm not in ext_files and not any(cat_of(k) and k.split("/")[-1] == nm.split("/")[-1] for k in ext_files):
            ext_files[nm].add("")
            ext_spec.setdefault(nm, nm)
    for P in procs:   # the tool a process is run with (gunicorn, celery ...) is part of the stack too
        for c in P["chain"]:
            tool = posixpath.basename((re.findall(r"[^\s\"'\[\],]+", c["cmd"]) or [""])[0])
            if cat_of(tool):
                ext_files[tool].add(c["file"])
                ext_spec.setdefault(tool, tool)
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
    main_lang = next((l for l, _ in langs.most_common() if not plugins[l].markup), langs.most_common(1)[0][0])
    summary = ""
    if "" in readme_dir:
        summary = read_para(root / readme_dir[""])
    if not summary:
        summary = f"A {main_lang} {kind} with {len(units)} main parts arranged in {len(layers_out)} layers."
    repo = dict(name=name, summary=summary, kind=f"{main_lang} {kind}", files=len(F), loc=sum(langs.values()),
                parts=len(units), deps=len(ext_files), langs=[[l, c] for l, c in langs.most_common(6)],
                routes=sum(1 for f in F.values() if f["layer"] != "tests" for s in f["symbols"] if s["k"] == "route"),
                stack=stack, libs=libs, entries=entries, generated=date.today().isoformat(),
                runs=[P["name"] for P in procs] if entry_kind == "Entry points" else [],
                colors={n: plugins[n].color for n in langs})

    for n in nodes.values():
        n.pop("_files", None)
    return dict(repo=repo, layers=layers_out, nodes=nodes, flow=flow, io={k: [lab, way] for k, lab, way, _ in IO})


# ════════════════════════════════ 7. render ════════════════════════════════

def render(data: dict, title: str) -> str:
    import html as _html
    tpl = pkgutil.get_data(__package__, "viewer.html").decode("utf-8")
    # every "<" escaped: "</script>" or "<!--<script" in a file name can then never end or derail the <script> block
    blob = json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("<", "\\u003c")
    return tpl.replace("__TITLE__", _html.escape(title)).replace("/*__DATA__*/null", blob)
