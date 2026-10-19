"""One small fixture repository per language: checks symbols, entry points and import links.

Run:  python -m unittest discover -s tests -v      (from the project folder)
"""
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from repoatlas import core, languages      # noqa: E402


def build(files):
    """Write `files` into a temp folder, analyse it, return the node table."""
    with tempfile.TemporaryDirectory() as d:
        for rel, text in files.items():
            p = Path(d) / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(textwrap.dedent(text))
        return core.analyze(Path(d), "fixture")["nodes"]


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


if __name__ == "__main__":
    unittest.main()
