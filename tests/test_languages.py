"""One small fixture repository per language: checks symbols, entry points and import links.

Run:  python -m unittest discover -s tests -v      (from the project folder)
"""
import re
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from repoatlas import core, languages      # noqa: E402


def analyze(files):
    """Write `files` into a temp folder and analyse it."""
    with tempfile.TemporaryDirectory() as d:
        for rel, text in files.items():
            p = Path(d) / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(textwrap.dedent(text))
        return core.analyze(Path(d), "fixture")


def build(files):
    """The node table of `files`, analysed."""
    return analyze(files)["nodes"]


def names(node):
    return {s["n"] for s in node["symbols"]}


class Registry(unittest.TestCase):
    def test_every_language_is_registered_once(self):
        exts = [e for c in languages.ALL for e in c.extensions]
        self.assertEqual(len(exts), len(set(exts)))
        for c in languages.ALL:
            self.assertTrue(c.name and c.extensions and c.color.startswith("#"), c)
        self.assertEqual(languages.BY_EXT[".ts"].name, "TypeScript")
        self.assertEqual(languages.BY_EXT[".hpp"].name, "C++")

    def test_one_file_per_language(self):
        folder = Path(languages.__file__).parent
        for c in languages.ALL:
            self.assertTrue((folder / (c.__module__.rsplit(".", 1)[-1] + ".py")).exists(), c)


class Python(unittest.TestCase):
    def test_imports_symbols_and_libraries(self):
        n = build({
            "app/__init__.py": "",
            "app/main.py": '"""Starts the app."""\nfrom app.models import User\nfrom . import util\nimport requests, os\n'
                           'if __name__ == "__main__":\n    pass\n',
            "app/models.py": "class User(Base):\n    '''A user.'''\n    def save(self): pass\n",
            "app/util.py": "def helper(a, b): pass\n",
        })
        self.assertEqual(sorted(n["app/main.py"]["uses"]), ["app/models.py", "app/util.py"])
        self.assertEqual(n["app/main.py"]["ext"], ["requests"])
        self.assertTrue(n["app/main.py"]["entry"])
        self.assertEqual(n["app/main.py"]["desc"], "Starts the app.")
        self.assertEqual(n["app/models.py"]["symbols"][0]["members"], ["save"])

    def test_unparseable_python_still_gets_its_imports(self):
        n = build({
            "legacy.py": "from . import helpers\nfrom pkg.mod import (a,\n    b)\nimport os, requests as r\nprint 'python 2'\n",
            "helpers.py": "x = 1\n",
            "pkg/__init__.py": "",
            "pkg/mod.py": "a = b = 1\n",
        })
        self.assertEqual(sorted(n["legacy.py"]["uses"]), ["helpers.py", "pkg/mod.py"])
        self.assertEqual(n["legacy.py"]["ext"], ["requests"])

    def test_standard_library_is_never_an_outside_library(self):
        # includes modules that are new (tomllib), old (distutils) and ones missing from naive lists (codecs)
        n = build({"a.py": "import codecs, contextvars, tomllib, distutils, concurrent.futures, requests\n"})
        self.assertEqual(n["a.py"]["ext"], ["requests"])

    def test_web_routes(self):
        n = build({"api.py": "from flask import Flask\napp = Flask(__name__)\n@app.get('/users')\ndef users(): pass\n"})
        self.assertEqual([(s["sig"], s["n"]) for s in n["api.py"]["symbols"]], [("GET", "/users")])
        self.assertEqual(n["api.py"]["layer"], "api")


class JavaScriptFamily(unittest.TestCase):
    def test_javascript(self):
        n = build({
            "package.json": '{"name": "x", "main": "src/index.js"}',
            "src/index.js": "import a from './a'\nconst b = require('./lib/b')\nimport React from 'react'\nimport fs from 'fs'\n",
            "src/a.js": "export default function Alpha() {}\n",
            "src/lib/b/index.js": "module.exports = 2\n",
        })
        self.assertEqual(sorted(n["src/index.js"]["uses"]), ["src/a.js", "src/lib/b/index.js"])
        self.assertEqual(n["src/index.js"]["ext"], ["react"])        # fs is built in, not a library
        self.assertTrue(n["src/index.js"]["entry"])                   # named by package.json "main"
        self.assertIn("Alpha", names(n["src/a.js"]))

    def test_typescript_alias_and_js_suffix(self):
        n = build({
            "src/app.ts": "import { u } from '@/util'\nimport type { T } from './types.js'\nexport const run = () => 1\n",
            "src/util.ts": "export const u = 1\n",
            "src/types.ts": "export interface T {}\n",
            "src/globals.d.ts": "declare const X: number\n",
        })
        self.assertEqual(sorted(n["src/app.ts"]["uses"]), ["src/types.ts", "src/util.ts"])
        self.assertNotIn("src/globals.d.ts", n)                       # declaration files are skipped
        self.assertIn("run", names(n["src/app.ts"]))

    def test_vue_and_svelte(self):
        n = build({
            "src/App.vue": "<script>\nimport Btn from './Btn.vue'\n</script>\n<template><Btn/></template>\n",
            "src/Btn.vue": "<template><button/></template>\n",
            "src/Page.svelte": "<script>\n  import Card from './Card.svelte'\n</script>\n<Card/>\n",
            "src/Card.svelte": "<div>card</div>\n",
        })
        self.assertEqual(n["src/App.vue"]["uses"], ["src/Btn.vue"])
        self.assertEqual(n["src/Page.svelte"]["uses"], ["src/Card.svelte"])
        self.assertEqual(n["src/App.vue"]["layer"], "interface")


class Go(unittest.TestCase):
    def test_module_imports(self):
        n = build({
            "go.mod": "module example.com/app\n",
            "main.go": 'package main\n\nimport (\n\t"fmt"\n\t"example.com/app/util"\n\t"github.com/x/y"\n)\n\nfunc main() {}\n',
            "util/util.go": "package util\n\n// Help does it.\nfunc Help() {}\n",
        })
        self.assertEqual(n["main.go"]["uses"], ["util/util.go"])
        self.assertEqual(n["main.go"]["ext"], ["github.com/x/y"])     # fmt is standard library
        self.assertTrue(n["main.go"]["entry"])
        self.assertEqual(n["util/util.go"]["symbols"][0]["doc"], "Help does it.")


class Jvm(unittest.TestCase):
    def test_java(self):
        n = build({
            "src/main/java/com/acme/App.java": "package com.acme;\nimport com.acme.util.Helper;\nimport org.springframework.boot.SpringApplication;\n"
                                               "import java.util.List;\npublic class App {\n  public static void main(String[] a) {}\n}\n",
            "src/main/java/com/acme/util/Helper.java": "package com.acme.util;\npublic class Helper {}\n",
        })
        app = n["src/main/java/com/acme/App.java"]
        self.assertEqual(app["uses"], ["src/main/java/com/acme/util/Helper.java"])
        self.assertEqual(app["ext"], ["org.springframework.boot"])
        self.assertTrue(app["entry"])
        self.assertIn("App", names(app))

    def test_kotlin(self):
        n = build({
            "src/App.kt": "package app\nimport app.util.Tools\nimport io.ktor.server.Netty\nfun main() {}\nobject Registry\n",
            "src/Tools.kt": "package app.util\nclass Tools\n",
        })
        app = n["src/App.kt"]
        self.assertEqual(app["uses"], ["src/Tools.kt"])
        self.assertEqual(app["ext"], ["io.ktor.server"])
        self.assertTrue(app["entry"])
        self.assertTrue({"main", "Registry"} <= names(app))


class Rust(unittest.TestCase):
    def test_mods_and_use(self):
        n = build({
            "Cargo.toml": '[package]\nname = "demo"\n',
            "src/main.rs": "mod util;\nmod net;\nuse crate::util::helper;\nuse serde::Serialize;\nuse std::fmt;\nfn main() {}\n",
            "src/util.rs": "pub fn helper() {}\n",
            "src/net/mod.rs": "pub mod client;\n",
            "src/net/client.rs": "use crate::util::helper;\nuse super::client as c;\npub struct Client;\n",
        })
        self.assertEqual(sorted(n["src/main.rs"]["uses"]), ["src/net/mod.rs", "src/util.rs"])
        self.assertEqual(n["src/main.rs"]["ext"], ["serde"])
        self.assertTrue(n["src/main.rs"]["entry"])
        self.assertEqual(n["src/net/mod.rs"]["uses"], ["src/net/client.rs"])
        self.assertIn("src/util.rs", n["src/net/client.rs"]["uses"])
        self.assertIn("Client", names(n["src/net/client.rs"]))


class CFamily(unittest.TestCase):
    def test_c(self):
        n = build({
            "src/main.c": '#include "util.h"\n#include <stdio.h>\nint main(void) { return 0; }\n',
            "src/util.h": "struct Thing { int a; };\n",
        })
        self.assertEqual(n["src/main.c"]["uses"], ["src/util.h"])
        self.assertTrue(n["src/main.c"]["entry"])
        self.assertIn("Thing", names(n["src/util.h"]))

    def test_cpp(self):
        n = build({"main.cpp": '#include "lib/util.hpp"\nint main() {}\n', "lib/util.hpp": "class Util {};\n"})
        self.assertEqual(n["main.cpp"]["uses"], ["lib/util.hpp"])
        self.assertEqual(n["main.cpp"]["lang"], "C++")


class SymbolOnlyLanguages(unittest.TestCase):
    def test_ruby_php_csharp_swift_scala(self):
        n = build({
            "a.rb": "class Foo\n  def bar\n  end\nend\n",
            "a.php": "<?php\nclass Foo {}\nfunction bar() {}\n",
            "A.cs": "public class Foo {}\n",
            "a.swift": "struct Foo {}\nfunc bar() {}\n",
            "A.scala": "trait Foo\nobject X {\n  def bar = 1\n}\n",
        })
        for path in ("a.rb", "a.php", "A.cs", "a.swift", "A.scala"):
            self.assertIn("Foo", names(n[path]), path)

    def test_html_and_css_are_interface(self):
        n = build({"index.html": "<html></html>\n", "site.css": "body { margin: 0 }\n", "x.py": "x = 1\n"})
        self.assertEqual(n["index.html"]["layer"], "interface")
        self.assertEqual(n["site.css"]["layer"], "interface")


class Generic(unittest.TestCase):
    """Links, entry points and inputs / outputs that work the same for every language."""

    def test_go_files_of_one_package_are_linked_by_name(self):
        n = build({
            "cmd/main.go": "package main\n\nfunc main() {\n\tquiz := loadQuiz()\n\t_ = quiz\n}\n",
            "cmd/quiz.go": "package main\n\nfunc loadQuiz() []string { return nil }\n",
            "other/quiz.go": "package other\n\nfunc loadQuiz() {}\n",      # same name, other package
        })
        self.assertEqual(n["cmd/main.go"]["uses"], ["cmd/quiz.go"])

    def test_csharp_is_linked_by_name_across_folders(self):
        n = build({
            "Program.cs": "class Program {\n  static void Main(string[] a) { new Greeter().Hi(); }\n}\n",
            "lib/Greeter.cs": "public class Greeter {\n  public void Hi() {}\n}\n",
        })
        self.assertEqual(n["Program.cs"]["uses"], ["lib/Greeter.cs"])
        self.assertTrue(n["Program.cs"]["entry"])

    def test_file_paths_in_code_are_links(self):
        n = build({
            "app.py": "import os\nTEMPLATE = os.path.join('templates', 'x')\npage = open('templates/page.html').read()\n",
            "templates/page.html": "<html></html>\n",
            "web/index.html": '<script src="main.js"></script>\n',
            "web/main.js": "console.log(1)\n",
        })
        self.assertEqual(n["app.py"]["uses"], ["templates/page.html"])
        self.assertEqual(n["web/index.html"]["uses"], ["web/main.js"])

    def test_comments_and_docstrings_are_not_code(self):
        n = build({
            "notes.py": '"""Mentions helper.py and @app.get("/x") but uses neither."""\n# see helper.py\nx = 1\n',
            "helper.py": "y = 2\n",
        })
        self.assertEqual(n["notes.py"]["uses"], [])
        self.assertNotEqual(n["notes.py"]["layer"], "api")

    def test_run_files_name_the_entry_points(self):
        n = build({
            "Procfile": "web: python server/run.py\n",
            "server/run.py": "x = 1\n",
            "svc/Dockerfile": 'FROM node\nCMD ["node", "src/start.js"]\n',
            "svc/src/start.js": "const a = 1\n",
            "Makefile": "run:\n\tpython -m tools.cli\n",
            "tools/__init__.py": "",
            "tools/cli.py": "x = 1\n",
        })
        for p in ("server/run.py", "svc/src/start.js", "tools/cli.py"):
            self.assertTrue(n[p]["entry"], p)

    def test_a_container_entrypoint_that_picks_a_command(self):
        data = analyze({
            "Dockerfile": 'FROM python\nWORKDIR /app\nCOPY start.py /app/entrypoint\nCOPY ./code /app/code\n'
                          'ENTRYPOINT ["/app/entrypoint"]\n',
            "start.py": 'import os, sys\nCMDS = {\n    "server": "gunicorn proj.wsgi --bind 0.0.0.0:8000",\n'
                        '    "worker": "celery -A proj worker",\n}\nos.chdir("/app/code")\n'
                        'os.execvp("sh", CMDS[sys.argv[1]].split())\n',
            "code/proj/__init__.py": "from .tasks import app\n",
            "code/proj/tasks.py": "app = None\n",
            "code/proj/wsgi.py": 'import os\nos.environ.setdefault("SETTINGS", "proj.settings")\n',
            "code/proj/settings.py": 'ROOT = "proj.urls"\n',
            "code/proj/urls.py": "x = 1\n",
            "code/worker/jobs.py": "x = 1\n",   # named like the `worker` argument, but not a module: not a target
        })
        P = {p["name"]: p for p in data["flow"]["processes"]}
        self.assertEqual(sorted(P), ["server", "worker"])
        self.assertEqual(P["server"]["entries"], ["code/proj/wsgi.py"])
        self.assertEqual(P["server"]["chain"], ["start.py"])
        self.assertIn("Dockerfile runs start.py, which runs `gunicorn proj.wsgi`.", P["server"]["narrative"])
        self.assertEqual(P["worker"]["entries"], ["code/proj/__init__.py"])   # bare `proj`, found in the working folder
        self.assertTrue(data["nodes"]["start.py"]["entry"])

    def test_files_the_framework_loads_by_convention(self):
        data = analyze({
            "Procfile": "web: python run.py\n",
            "run.py": "import shop.models, cart.models\n",
            "shop/__init__.py": "x = 1\n",
            "shop/models.py": "x = 1\n",
            "shop/apps.py": "import shop.signals\n",
            "shop/signals.py": "x = 1\n",
            "shop/unused.py": "x = 1\n",
            "shop/commands/a.py": "x = 1\n",
            "shop/commands/b.py": "x = 1\n",
            "shop/commands/c.py": "x = 1\n",
            "cart/__init__.py": "x = 1\n",
            "cart/models.py": "x = 1\n",
            "cart/apps.py": "x = 1\n",
        })
        n = data["nodes"]
        self.assertIn("shop/apps.py", n["shop/__init__.py"]["uses"])        # C1: in both packages, imported by neither
        self.assertIn("shop/commands/a.py", n["shop/__init__.py"]["uses"])  # C2: a folder of look-alike files
        seen, todo = set(data["flow"]["processes"][0]["entries"]), list(data["flow"]["processes"][0]["entries"])
        for p in todo:
            for q in n[p]["uses"]:
                if q not in seen:
                    seen.add(q)
                    todo.append(q)
        self.assertTrue({"shop/apps.py", "shop/signals.py", "shop/commands/c.py", "cart/apps.py"} <= seen)
        self.assertNotIn("shop/unused.py", seen)                              # one unused file is just unused

    def test_a_tool_that_runs_the_project_starts_at_its_convention_files(self):
        data = analyze({
            "package.json": '{"name": "site", "scripts": {"dev": "next dev", "test": "jest", "format": "prettier --write ."},'
                            ' "dependencies": {"next": "14"}, "devDependencies": {"prettier": "3"}}',
            "pages/index.tsx": "import Nav from '../components/Nav'\nexport default function Home() { return <Nav/> }\n",
            "pages/about.tsx": "export default function About() { return null }\n",
            "pages/blog.tsx": "export default function Blog() { return null }\n",
            "components/Nav.tsx": "export default function Nav() { return null }\n",
        })
        P = data["flow"]["processes"]
        self.assertEqual([p["name"] for p in P], ["dev"])                    # "test" is not a running program
        self.assertEqual(sorted(P[0]["entries"]), ["pages/about.tsx", "pages/blog.tsx", "pages/index.tsx"])

    def test_one_process_per_program_without_launch_files(self):
        data = analyze({
            "a/main.go": "package main\n\nfunc main() { helpA() }\n",
            "a/help.go": "package main\n\nfunc helpA() {}\n",
            "b/main.go": "package main\n\nfunc main() {}\n",
        })
        self.assertEqual([p["name"] for p in data["flow"]["processes"]], ["a/main.go", "b/main.go"])
        self.assertEqual(data["repo"]["runs"], ["a/main.go", "b/main.go"])

    def test_no_framework_is_hard_coded(self):
        """Framework knowledge may live in data tables only, never in the logic."""
        src = Path(core.__file__).parent
        for path in [src / "core.py"] + sorted((src / "languages").glob("*.py")):
            text = core.COMMENTS.sub(" ", path.read_text())
            text = re.sub(r"\nEXT_CATS = \{.*?\n\}", "", text, flags=re.S)   # the library catalogue is data
            for word in ("django", "flask", "celery", "gunicorn", "uvicorn", "rails", "laravel", "lumen", "spring",
                         "INSTALLED_APPS", "execute_from_command_line", "manage.py"):
                self.assertIsNone(re.search(r"\b" + re.escape(word) + r"\b", text, re.I), f"{word} in {path.name}")

    def test_names_built_at_run_time_and_classes_named_in_strings(self):
        n = build({
            "cmd/main.go": 'package main\n\nimport "fmt"\n\nfunc main() { fmt.Println(fmt.Sprintf("html/ex%d.html", 1)) }\n',
            "cmd/html/ex1.html": "<p>1</p>\n",
            "cmd/html/ex2.html": "<p>2</p>\n",
            "routes/web.php": "<?php\n$app->post('login', 'AuthController@login');\nMail::send('emails.welcome', $d);\n",
            "app/AuthController.php": "<?php\nclass AuthController {}\n",
            "views/emails/welcome.blade.php": "<p>hi</p>\n",
        })
        self.assertEqual(n["cmd/main.go"]["uses"], ["cmd/html/ex1.html", "cmd/html/ex2.html"])
        self.assertEqual(n["routes/web.php"]["uses"], ["app/AuthController.php", "views/emails/welcome.blade.php"])

    def test_compose_command_and_test_helpers(self):
        data = analyze({
            "docker-compose.yml": "services:\n  web:\n    command: python -m app.server\n",
            "app/__init__.py": "",
            "app/server.py": "x = 1\n",
            "conftest.py": "import pytest\n",
            "shop/factories.py": "class OrderFactory: pass\n",
        })
        n = data["nodes"]
        self.assertEqual([p["name"] for p in data["flow"]["processes"]], ["web"])
        self.assertTrue(n["app/server.py"]["entry"])
        self.assertEqual(n["conftest.py"]["layer"], "tests")
        self.assertEqual(n["shop/factories.py"]["layer"], "tests")

    def test_shell_scripts(self):
        n = build({
            "install.sh": '#!/bin/sh\n. ./lib.sh\nsetup_dir() {\n  mkdir -p "$DIR"\n}\ncurl -fsSL "${URL:-https://x}"\n',
            "lib.sh": "say() { echo hi; }\n",
        })
        self.assertTrue(n["install.sh"]["entry"])
        self.assertEqual(n["install.sh"]["uses"], ["lib.sh"])
        self.assertIn("setup_dir", names(n["install.sh"]))
        self.assertEqual(n["install.sh"]["io"], ["env", "http_out"])

    def test_inputs_and_outputs_reach_the_flow(self):
        data = analyze({
            "main.py": "import sys\nfrom work import go\nif __name__ == '__main__':\n    go(sys.argv[1])\n",
            "work.py": "from pathlib import Path\ndef go(p):\n    Path(p).write_text('done')\n",
        })
        self.assertEqual(data["nodes"]["main.py"]["io"], ["args"])
        self.assertEqual(data["nodes"]["work.py"]["io"], ["file_out"])
        P = data["flow"]["processes"][0]
        self.assertEqual(P["io"], {"args": ["main.py"], "file_out": ["work.py"]})
        self.assertIn("It takes in command-line arguments, and gives out files.", P["narrative"])
        self.assertEqual(data["io"]["file_out"], ["Files", "out"])


if __name__ == "__main__":
    unittest.main()
