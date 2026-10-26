//! One small fixture repository per case: symbols, entry points, import links, programs and their journeys.
//!
//! Run:  cargo test

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Write `files` into a temp folder and analyse it.
fn analyze(files: &[(&str, &str)]) -> Value {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let p = d.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    repoatlas::analyze(d.path(), "fixture").unwrap()
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn uses(d: &Value, p: &str) -> Vec<String> {
    sorted(strs(&d["nodes"][p]["uses"]))
}

fn names(d: &Value, p: &str) -> Vec<String> {
    d["nodes"][p]["symbols"].as_array().unwrap().iter().map(|s| s["n"].as_str().unwrap().to_string()).collect()
}

fn procs(d: &Value) -> Vec<Value> {
    d["flow"]["processes"].as_array().cloned().unwrap_or_default()
}

fn proc_named(d: &Value, name: &str) -> Value {
    procs(d).into_iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("no process {name}: {:?}", proc_names(d)))
}

fn proc_names(d: &Value) -> Vec<String> {
    procs(d).iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

/// (depth, kind, name) of every step
fn steps(p: &Value) -> Vec<(u64, String, String)> {
    p["steps"].as_array().unwrap().iter()
        .map(|s| (s["d"].as_u64().unwrap(), s["k"].as_str().unwrap().to_string(), s["n"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn fallback_summary_has_no_layers() {
    let d = analyze(&[("a.py", "def a():\n    return 1\n"), ("b.py", "from a import a\n\ndef b():\n    return a()\n")]);
    let s = d["repo"]["summary"].as_str().unwrap();
    assert!(!s.contains("layers") && s.contains("main parts"), "{s}");
}

// ---------------------------------------------------------------- the language table

#[test]
fn every_language_is_registered_once() {
    let mut exts: Vec<&str> = repoatlas::lang::LANGS.iter().flat_map(|l| l.exts.iter().copied()).collect();
    let n = exts.len();
    exts.sort();
    exts.dedup();
    assert_eq!(n, exts.len());
    for l in repoatlas::lang::LANGS {
        assert!(!l.name.is_empty() && !l.exts.is_empty() && l.color.starts_with('#'), "{}", l.name);
    }
    assert_eq!(repoatlas::lang::by_ext(".ts").unwrap().name, "TypeScript");
    assert_eq!(repoatlas::lang::by_ext(".hpp").unwrap().name, "C++");
}

#[test]
fn every_query_compiles() {
    for l in repoatlas::lang::LANGS {
        if let Some(g) = l.grammar {
            assert!(repoatlas::lang::GRAMMARS.contains_key(g), "{}", l.name);
        }
    }
}

// ---------------------------------------------------------------- per language

#[test]
fn python_imports_symbols_and_libraries() {
    let d = analyze(&[
        ("app/__init__.py", ""),
        ("app/main.py", "\"\"\"Starts the app.\"\"\"\nfrom app.models import User\nfrom . import util\nimport requests, os\nif __name__ == \"__main__\":\n    pass\n"),
        ("app/models.py", "class User(Base):\n    '''A user.'''\n    def save(self): pass\n"),
        ("app/util.py", "def helper(a, b): pass\n"),
    ]);
    assert_eq!(uses(&d, "app/main.py"), ["app/models.py", "app/util.py"]);
    assert_eq!(strs(&d["nodes"]["app/main.py"]["ext"]), ["requests"]);
    assert_eq!(d["nodes"]["app/main.py"]["entry"], true);
    assert_eq!(d["nodes"]["app/main.py"]["desc"], "Starts the app.");
    assert_eq!(strs(&d["nodes"]["app/models.py"]["symbols"][0]["members"]), ["save"]);
}

#[test]
fn unparseable_python_still_gets_its_imports() {
    let d = analyze(&[
        ("legacy.py", "from . import helpers\nfrom pkg.mod import (a,\n    b)\nimport os, requests as r\nprint 'python 2'\n"),
        ("helpers.py", "x = 1\n"),
        ("pkg/__init__.py", ""),
        ("pkg/mod.py", "a = b = 1\n"),
    ]);
    assert_eq!(uses(&d, "legacy.py"), ["helpers.py", "pkg/mod.py"]);
    assert_eq!(strs(&d["nodes"]["legacy.py"]["ext"]), ["requests"]);
}

#[test]
fn standard_library_is_never_an_outside_library() {
    // includes modules that are new (tomllib), old (distutils) and ones missing from naive lists (codecs)
    let d = analyze(&[("a.py", "import codecs, contextvars, tomllib, distutils, concurrent.futures, requests\n")]);
    assert_eq!(strs(&d["nodes"]["a.py"]["ext"]), ["requests"]);
}

#[test]
fn python_web_routes() {
    let d = analyze(&[("api.py", "from flask import Flask\napp = Flask(__name__)\n@app.get('/users')\ndef users(): pass\n")]);
    let s = &d["nodes"]["api.py"]["symbols"];
    assert_eq!((s[0]["sig"].as_str(), s[0]["n"].as_str(), s.as_array().unwrap().len()), (Some("GET"), Some("/users"), 1));
    assert_eq!(d["nodes"]["api.py"]["layer"], "api");
}

#[test]
fn javascript() {
    let d = analyze(&[
        ("package.json", r#"{"name": "x", "main": "src/index.js"}"#),
        ("src/index.js", "import a from './a'\nconst b = require('./lib/b')\nimport React from 'react'\nimport fs from 'fs'\n"),
        ("src/a.js", "export default function Alpha() {}\n"),
        ("src/lib/b/index.js", "module.exports = 2\n"),
    ]);
    assert_eq!(uses(&d, "src/index.js"), ["src/a.js", "src/lib/b/index.js"]);
    assert_eq!(strs(&d["nodes"]["src/index.js"]["ext"]), ["react"]); // fs is built in, not a library
    assert_eq!(d["nodes"]["src/index.js"]["entry"], true); // named by package.json "main"
    assert!(names(&d, "src/a.js").contains(&"Alpha".to_string()));
}

#[test]
fn typescript_alias_and_js_suffix() {
    let d = analyze(&[
        ("src/app.ts", "import { u } from '@/util'\nimport type { T } from './types.js'\nexport const run = () => 1\n"),
        ("src/util.ts", "export const u = 1\n"),
        ("src/types.ts", "export interface T {}\n"),
        ("src/globals.d.ts", "declare const X: number\n"),
    ]);
    assert_eq!(uses(&d, "src/app.ts"), ["src/types.ts", "src/util.ts"]);
    assert!(d["nodes"].get("src/globals.d.ts").is_none()); // declaration files are skipped
    assert!(names(&d, "src/app.ts").contains(&"run".to_string()));
}

#[test]
fn vue_and_svelte() {
    let d = analyze(&[
        ("src/App.vue", "<script>\nimport Btn from './Btn.vue'\n</script>\n<template><Btn/></template>\n"),
        ("src/Btn.vue", "<template><button/></template>\n"),
        ("src/Page.svelte", "<script>\n  import Card from './Card.svelte'\n</script>\n<Card/>\n"),
        ("src/Card.svelte", "<div>card</div>\n"),
    ]);
    assert_eq!(uses(&d, "src/App.vue"), ["src/Btn.vue"]);
    assert_eq!(uses(&d, "src/Page.svelte"), ["src/Card.svelte"]);
    assert_eq!(d["nodes"]["src/App.vue"]["layer"], "interface");
}

#[test]
fn go_module_imports() {
    let d = analyze(&[
        ("go.mod", "module example.com/app\n"),
        ("main.go", "package main\n\nimport (\n\t\"fmt\"\n\t\"example.com/app/util\"\n\t\"github.com/x/y\"\n)\n\nfunc main() {}\n"),
        ("util/util.go", "package util\n\n// Help does it.\nfunc Help() {}\n"),
    ]);
    assert_eq!(uses(&d, "main.go"), ["util/util.go"]);
    assert_eq!(strs(&d["nodes"]["main.go"]["ext"]), ["github.com/x/y"]); // fmt is standard library
    assert_eq!(d["nodes"]["main.go"]["entry"], true);
    assert_eq!(d["nodes"]["util/util.go"]["symbols"][0]["doc"], "Help does it.");
}

#[test]
fn java() {
    let d = analyze(&[
        ("src/main/java/com/acme/App.java", "package com.acme;\nimport com.acme.util.Helper;\nimport org.springframework.boot.SpringApplication;\nimport java.util.List;\npublic class App {\n  public static void main(String[] a) {}\n}\n"),
        ("src/main/java/com/acme/util/Helper.java", "package com.acme.util;\npublic class Helper {}\n"),
    ]);
    let app = "src/main/java/com/acme/App.java";
    assert_eq!(uses(&d, app), ["src/main/java/com/acme/util/Helper.java"]);
    assert_eq!(strs(&d["nodes"][app]["ext"]), ["org.springframework.boot"]);
    assert_eq!(d["nodes"][app]["entry"], true);
    assert!(names(&d, app).contains(&"App".to_string()));
}

#[test]
fn kotlin() {
    let d = analyze(&[
        ("src/App.kt", "package app\nimport app.util.Tools\nimport io.ktor.server.Netty\nfun main() {}\nobject Registry\n"),
        ("src/Tools.kt", "package app.util\nclass Tools\n"),
    ]);
    assert_eq!(uses(&d, "src/App.kt"), ["src/Tools.kt"]);
    assert_eq!(strs(&d["nodes"]["src/App.kt"]["ext"]), ["io.ktor.server"]);
    assert_eq!(d["nodes"]["src/App.kt"]["entry"], true);
    let n = names(&d, "src/App.kt");
    assert!(n.contains(&"main".to_string()) && n.contains(&"Registry".to_string()), "{n:?}");
}

#[test]
fn rust_mods_and_use() {
    let d = analyze(&[
        ("Cargo.toml", "[package]\nname = \"demo\"\n"),
        ("src/main.rs", "mod util;\nmod net;\nuse crate::util::helper;\nuse serde::Serialize;\nuse std::fmt;\nfn main() {}\n"),
        ("src/util.rs", "pub fn helper() {}\n"),
        ("src/net/mod.rs", "pub mod client;\n"),
        ("src/net/client.rs", "use crate::util::helper;\nuse super::client as c;\npub struct Client;\n"),
    ]);
    assert_eq!(uses(&d, "src/main.rs"), ["src/net/mod.rs", "src/util.rs"]);
    assert_eq!(strs(&d["nodes"]["src/main.rs"]["ext"]), ["serde"]);
    assert_eq!(d["nodes"]["src/main.rs"]["entry"], true);
    assert_eq!(uses(&d, "src/net/mod.rs"), ["src/net/client.rs"]);
    assert!(uses(&d, "src/net/client.rs").contains(&"src/util.rs".to_string()));
    assert!(names(&d, "src/net/client.rs").contains(&"Client".to_string()));
}

#[test]
fn c_and_cpp() {
    let d = analyze(&[
        ("src/main.c", "#include \"util.h\"\n#include <stdio.h>\nint main(void) { return 0; }\n"),
        ("src/util.h", "struct Thing { int a; };\n"),
    ]);
    assert_eq!(uses(&d, "src/main.c"), ["src/util.h"]);
    assert_eq!(d["nodes"]["src/main.c"]["entry"], true);
    assert!(names(&d, "src/util.h").contains(&"Thing".to_string()));
    let d = analyze(&[("main.cpp", "#include \"lib/util.hpp\"\nint main() {}\n"), ("lib/util.hpp", "class Util {};\n")]);
    assert_eq!(uses(&d, "main.cpp"), ["lib/util.hpp"]);
    assert_eq!(d["nodes"]["main.cpp"]["lang"], "C++");
}

#[test]
fn ruby_php_csharp_swift_scala() {
    let d = analyze(&[
        ("a.rb", "class Foo\n  def bar\n  end\nend\n"),
        ("a.php", "<?php\nclass Foo {}\nfunction bar() {}\n"),
        ("A.cs", "public class Foo {}\n"),
        ("a.swift", "struct Foo {}\nfunc bar() {}\n"),
        ("A.scala", "trait Foo\nobject X {\n  def bar = 1\n}\n"),
    ]);
    for p in ["a.rb", "a.php", "A.cs", "a.swift", "A.scala"] {
        assert!(names(&d, p).contains(&"Foo".to_string()), "{p}");
    }
}

#[test]
fn html_and_css_are_interface() {
    let d = analyze(&[("index.html", "<html></html>\n"), ("site.css", "body { margin: 0 }\n"), ("x.py", "x = 1\n")]);
    assert_eq!(d["nodes"]["index.html"]["layer"], "interface");
    assert_eq!(d["nodes"]["site.css"]["layer"], "interface");
}

// ---------------------------------------------------------------- links, entry points and I/O that work the same everywhere

#[test]
fn go_files_of_one_package_are_linked_by_name() {
    let d = analyze(&[
        ("cmd/main.go", "package main\n\nfunc main() {\n\tquiz := loadQuiz()\n\t_ = quiz\n}\n"),
        ("cmd/quiz.go", "package main\n\nfunc loadQuiz() []string { return nil }\n"),
        ("other/quiz.go", "package other\n\nfunc loadQuiz() {}\n"), // same name, other package
    ]);
    assert_eq!(uses(&d, "cmd/main.go"), ["cmd/quiz.go"]);
}

#[test]
fn csharp_is_linked_by_name_across_folders() {
    let d = analyze(&[
        ("Program.cs", "class Program {\n  static void Main(string[] a) { new Greeter().Hi(); }\n}\n"),
        ("lib/Greeter.cs", "public class Greeter {\n  public void Hi() {}\n}\n"),
    ]);
    assert_eq!(uses(&d, "Program.cs"), ["lib/Greeter.cs"]);
    assert_eq!(d["nodes"]["Program.cs"]["entry"], true);
}

#[test]
fn file_paths_in_code_are_links() {
    let d = analyze(&[
        ("app.py", "import os\nTEMPLATE = os.path.join('templates', 'x')\npage = open('templates/page.html').read()\n"),
        ("templates/page.html", "<html></html>\n"),
        ("web/index.html", "<script src=\"main.js\"></script>\n"),
        ("web/main.js", "console.log(1)\n"),
    ]);
    assert_eq!(uses(&d, "app.py"), ["templates/page.html"]);
    assert_eq!(uses(&d, "web/index.html"), ["web/main.js"]);
}

#[test]
fn comments_and_docstrings_are_not_code() {
    let d = analyze(&[
        ("notes.py", "\"\"\"Mentions helper.py and @app.get(\"/x\") but uses neither.\"\"\"\n# see helper.py\nx = 1\n"),
        ("helper.py", "y = 2\n"),
    ]);
    assert!(uses(&d, "notes.py").is_empty());
    assert_ne!(d["nodes"]["notes.py"]["layer"], "api");
}

#[test]
fn run_files_name_the_entry_points() {
    let d = analyze(&[
        ("Procfile", "web: python server/run.py\n"),
        ("server/run.py", "x = 1\n"),
        ("svc/Dockerfile", "FROM node\nCMD [\"node\", \"src/start.js\"]\n"),
        ("svc/src/start.js", "const a = 1\n"),
        ("Makefile", "run:\n\tpython -m tools.cli\n"),
        ("tools/__init__.py", ""),
        ("tools/cli.py", "x = 1\n"),
    ]);
    for p in ["server/run.py", "svc/src/start.js", "tools/cli.py"] {
        assert_eq!(d["nodes"][p]["entry"], true, "{p}");
    }
}

#[test]
fn a_container_entrypoint_that_picks_a_command() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nWORKDIR /app\nCOPY start.py /app/entrypoint\nCOPY ./code /app/code\nENTRYPOINT [\"/app/entrypoint\"]\n"),
        ("start.py", "import os, sys\nCMDS = {\n    \"server\": \"gunicorn proj.wsgi --bind 0.0.0.0:8000\",\n    \"worker\": \"celery -A proj worker\",\n}\nos.chdir(\"/app/code\")\nos.execvp(\"sh\", CMDS[sys.argv[1]].split())\n"),
        ("code/proj/__init__.py", "from .tasks import app\n"),
        ("code/proj/tasks.py", "app = None\n"),
        ("code/proj/wsgi.py", "import os\nos.environ.setdefault(\"SETTINGS\", \"proj.settings\")\n"),
        ("code/proj/settings.py", "ROOT = \"proj.urls\"\n"),
        ("code/proj/urls.py", "x = 1\n"),
        ("code/worker/jobs.py", "x = 1\n"), // named like the `worker` argument, but not a module: not a target
    ]);
    assert_eq!(sorted(proc_names(&d)), ["server", "worker"]);
    let server = proc_named(&d, "server");
    assert_eq!(strs(&server["entries"]), ["code/proj/wsgi.py"]);
    assert_eq!(strs(&server["chain"]), ["start.py"]);
    assert!(server["narrative"].as_str().unwrap().contains("Dockerfile runs start.py, which runs `gunicorn proj.wsgi`."), "{}", server["narrative"]);
    assert_eq!(strs(&proc_named(&d, "worker")["entries"]), ["code/proj/__init__.py"]); // bare `proj`, found in the working folder
    assert_eq!(d["nodes"]["start.py"]["entry"], true);
}

#[test]
fn files_the_framework_loads_by_convention() {
    let d = analyze(&[
        ("Procfile", "web: python run.py\n"),
        ("run.py", "import shop.models, cart.models\n"),
        ("shop/__init__.py", "x = 1\n"),
        ("shop/models.py", "x = 1\n"),
        ("shop/apps.py", "import shop.signals\n"),
        ("shop/signals.py", "x = 1\n"),
        ("shop/unused.py", "x = 1\n"),
        ("shop/commands/a.py", "x = 1\n"),
        ("shop/commands/b.py", "x = 1\n"),
        ("shop/commands/c.py", "x = 1\n"),
        ("cart/__init__.py", "x = 1\n"),
        ("cart/models.py", "x = 1\n"),
        ("cart/apps.py", "x = 1\n"),
    ]);
    let init = uses(&d, "shop/__init__.py");
    assert!(init.contains(&"shop/apps.py".to_string())); // C1: in both packages, imported by neither
    assert!(init.contains(&"shop/commands/a.py".to_string())); // C2: a folder of look-alike files
    let mut seen: Vec<String> = strs(&procs(&d)[0]["entries"]);
    let mut i = 0;
    while i < seen.len() {
        for q in uses(&d, &seen[i].clone()) {
            if !seen.contains(&q) {
                seen.push(q);
            }
        }
        i += 1;
    }
    for p in ["shop/apps.py", "shop/signals.py", "shop/commands/c.py", "cart/apps.py"] {
        assert!(seen.contains(&p.to_string()), "{p}");
    }
    assert!(!seen.contains(&"shop/unused.py".to_string())); // one unused file is just unused
}

#[test]
fn a_tool_that_runs_the_project_starts_at_its_convention_files() {
    let d = analyze(&[
        ("package.json", r#"{"name": "site", "scripts": {"dev": "next dev", "test": "jest", "format": "prettier --write ."}, "dependencies": {"next": "14"}, "devDependencies": {"prettier": "3"}}"#),
        ("pages/index.tsx", "import Nav from '../components/Nav'\nexport default function Home() { return <Nav/> }\n"),
        ("pages/about.tsx", "export default function About() { return null }\n"),
        ("pages/blog.tsx", "export default function Blog() { return null }\n"),
        ("components/Nav.tsx", "export default function Nav() { return null }\n"),
    ]);
    assert_eq!(proc_names(&d), ["dev"]); // "test" is not a running program
    assert_eq!(sorted(strs(&procs(&d)[0]["entries"])), ["pages/about.tsx", "pages/blog.tsx", "pages/index.tsx"]);
}

#[test]
fn one_process_per_program_without_launch_files() {
    let d = analyze(&[
        ("a/main.go", "package main\n\nfunc main() { helpA() }\n"),
        ("a/help.go", "package main\n\nfunc helpA() {}\n"),
        ("b/main.go", "package main\n\nfunc main() {}\n"),
    ]);
    assert_eq!(proc_names(&d), ["a/main.go", "b/main.go"]);
    assert_eq!(strs(&d["repo"]["runs"]), ["a/main.go", "b/main.go"]);
}

#[test]
fn no_framework_is_hard_coded() {
    // framework knowledge may live in data tables only, never in the logic
    let comments = regex::Regex::new(r"//[^\n]*").unwrap();
    let catalogue = regex::Regex::new(r"(?s)\npub const EXT_CATS.*?\n\];").unwrap();
    for entry in fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") || path.file_name().is_some_and(|n| n == "rules.rs") {
            continue;
        }
        let text = comments.replace_all(&fs::read_to_string(&path).unwrap(), " ").to_string();
        let text = catalogue.replace_all(&text, "").to_string();
        for word in ["django", "flask", "celery", "gunicorn", "uvicorn", "rails", "laravel", "lumen", "spring", "INSTALLED_APPS",
                     "execute_from_command_line", "manage.py"] {
            let re = regex::Regex::new(&format!(r"(?i)\b{}\b", regex::escape(word))).unwrap();
            assert!(!re.is_match(&text), "{word} in {}", path.display());
        }
    }
}

#[test]
fn names_built_at_run_time_and_classes_named_in_strings() {
    let d = analyze(&[
        ("cmd/main.go", "package main\n\nimport \"fmt\"\n\nfunc main() { fmt.Println(fmt.Sprintf(\"html/ex%d.html\", 1)) }\n"),
        ("cmd/html/ex1.html", "<p>1</p>\n"),
        ("cmd/html/ex2.html", "<p>2</p>\n"),
        ("routes/web.php", "<?php\n$app->post('login', 'AuthController@login');\nMail::send('emails.welcome', $d);\n"),
        ("app/AuthController.php", "<?php\nclass AuthController {}\n"),
        ("views/emails/welcome.blade.php", "<p>hi</p>\n"),
    ]);
    assert_eq!(uses(&d, "cmd/main.go"), ["cmd/html/ex1.html", "cmd/html/ex2.html"]);
    assert_eq!(uses(&d, "routes/web.php"), ["app/AuthController.php", "views/emails/welcome.blade.php"]);
}

#[test]
fn compose_command_and_test_helpers() {
    let d = analyze(&[
        ("docker-compose.yml", "services:\n  web:\n    command: python -m app.server\n"),
        ("app/__init__.py", ""),
        ("app/server.py", "x = 1\n"),
        ("conftest.py", "import pytest\n"),
        ("shop/factories.py", "class OrderFactory: pass\n"),
    ]);
    assert_eq!(proc_names(&d), ["web"]);
    assert_eq!(d["nodes"]["app/server.py"]["entry"], true);
    assert_eq!(d["nodes"]["conftest.py"]["layer"], "tests");
    assert_eq!(d["nodes"]["shop/factories.py"]["layer"], "tests");
}

#[test]
fn shell_scripts() {
    let d = analyze(&[
        ("install.sh", "#!/bin/sh\n. ./lib.sh\nsetup_dir() {\n  mkdir -p \"$DIR\"\n}\ncurl -fsSL \"${URL:-https://x}\"\n"),
        ("lib.sh", "say() { echo hi; }\n"),
    ]);
    assert_eq!(d["nodes"]["install.sh"]["entry"], true);
    assert_eq!(uses(&d, "install.sh"), ["lib.sh"]);
    assert!(names(&d, "install.sh").contains(&"setup_dir".to_string()));
    assert_eq!(strs(&d["nodes"]["install.sh"]["io"]), ["env", "http_out"]);
}

#[test]
fn inputs_and_outputs_reach_the_flow() {
    let d = analyze(&[
        ("main.py", "import sys\nfrom work import go\nif __name__ == '__main__':\n    go(sys.argv[1])\n"),
        ("work.py", "from pathlib import Path\ndef go(p):\n    Path(p).write_text('done')\n"),
    ]);
    assert_eq!(strs(&d["nodes"]["main.py"]["io"]), ["args"]);
    assert_eq!(strs(&d["nodes"]["work.py"]["io"]), ["file_out"]);
    let p = &procs(&d)[0];
    assert_eq!(strs(&p["io"]["args"]), ["main.py"]);
    assert_eq!(strs(&p["io"]["file_out"]), ["work.py"]);
    assert_eq!(p["io"].as_object().unwrap().len(), 2);
    assert!(p["narrative"].as_str().unwrap().contains("It takes in command-line arguments, and gives out files."), "{}", p["narrative"]);
    assert_eq!(strs(&d["io"]["file_out"]), ["Files", "out"]);
}

#[test]
fn map_joins_units_across_programs() {
    let d = analyze(&[
        ("app/main.py", "from core.lib import work\n\nif __name__ == '__main__':\n    work()\n"),
        ("core/lib.py", "def work():\n    return 1\n"),
    ]);
    let m = &d["flow"]["map"];
    assert_eq!(sorted(strs(&m["parts"])), ["app", "core"]);
    assert_eq!(m["edges"], serde_json::json!([["app", "core", 1]]));
    assert_eq!(m["runs"], serde_json::json!([[[], ["app"]]]));
    assert!(strs(&d["flow"]["unreached"]).is_empty());
}

#[test]
fn map_is_empty_of_edges_for_one_unit() {
    let d = analyze(&[("main.py", "def a():\n    return b()\n\ndef b():\n    return 1\n\nif __name__ == '__main__':\n    a()\n")]);
    let p = &procs(&d)[0];
    assert_eq!(steps(p).len(), 3);
    assert_eq!(d["flow"]["map"]["edges"], serde_json::json!([]));
}

#[test]
fn map_has_one_box_per_program_and_one_map() {
    let d = analyze(&[
        ("a/main.py", "from core.lib import work\n\nif __name__ == '__main__':\n    work()\n"),
        ("b/main.py", "from core.lib import work\n\nif __name__ == '__main__':\n    work()\n"),
        ("core/lib.py", "def work():\n    return 1\n"),
    ]);
    assert_eq!(procs(&d).len(), 2);
    let m = &d["flow"]["map"];
    assert_eq!(m["runs"].as_array().unwrap().len(), 2);
    assert_eq!(sorted(strs(&m["parts"])), ["a", "b", "core"]);
    assert!(procs(&d).iter().all(|p| p.get("edges").is_none()));
    // each program has a row of its own; the part both use belongs to neither alone
    let lanes = m["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 2);
    assert!(strs(&lanes[0]).contains(&"core".to_string()) && strs(&lanes[1]).contains(&"core".to_string()));
}

#[test]
fn map_follows_calls_past_the_step_limit() {
    // a chain of 12 files: the journey stops at its depth limit, the map does not
    let mut files: Vec<(String, String)> = vec![("p0/main.py".into(), "from p1.m import f\n\nif __name__ == '__main__':\n    f()\n".into())];
    for i in 1..12 {
        let body = if i < 11 { format!("from p{}.m import f as g\n\ndef f():\n    return g()\n", i + 1) } else { "def f():\n    return 1\n".to_string() };
        files.push((format!("p{i}/m.py"), body));
    }
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let d = analyze(&refs);
    assert!(strs(&d["flow"]["map"]["parts"]).contains(&"p11".to_string()));
    assert!(!strs(&d["flow"]["unreached"]).contains(&"p11".to_string()));
}

#[test]
fn map_follows_routes_registered_when_a_file_loads() {
    // gunicorn app:app imports app.py, whose top level registers the routes; `source/` is the code root
    let d = analyze(&[
        ("runner.sh", "#!/bin/bash\nAPP_START_CMD=\"gunicorn app:app --bind 0.0.0.0:4023\"\nexec $APP_START_CMD\n"),
        ("source/app.py", "from api.routes import add_routes\n\napp = object()\nadd_routes(app)\n"),
        ("source/api/routes.py", "import api.users\nfrom api import items\n\n\ndef add_routes(app):\n    app.add_route(api.users.get, '/u')\n    app.add_route(items.get, '/i')\n"),
        ("source/api/users.py", "def get(request):\n    return helper()\n\n\ndef helper():\n    return 1\n"),
        ("source/api/items.py", "from models import save\n\n\ndef get(request):\n    return save()\n"),
        ("source/models/__init__.py", "def save():\n    return 1\n"),
    ]);
    let p = &procs(&d)[0];
    assert_eq!(strs(&p["entries"]), ["source/app.py"], "{:?}", proc_names(&d));
    let m = &d["flow"]["map"];
    let edges = m["edges"].as_array().unwrap();
    assert!(edges.iter().all(|e| e.as_array().unwrap().len() == 3), "calls, not imports: {m}");
    assert!(strs(&m["parts"]).iter().any(|u| u.starts_with("source/models")), "{m}");
}

#[test]
fn a_tests_folder_inside_a_part_does_not_make_it_tests() {
    let mut files: Vec<(String, String)> = vec![("main.py".into(), "from pkg.a import run\n\nif __name__ == '__main__':\n    run()\n".into())];
    files.push(("pkg/a.py".into(), "def run():\n    return 1\n".into()));
    files.push(("pkg/b.py".into(), "def other():\n    return 2\n".into()));
    for i in 0..4 {
        files.push((format!("pkg/tests/test_{i}.py"), "def test_it():\n    assert 1 == 1\n".repeat(20)));
    }
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let d = analyze(&refs);
    let m = &d["flow"]["map"];
    assert!(strs(&m["parts"]).iter().any(|u| u == "pkg" || u.starts_with("pkg/a")), "{m}");
}

#[test]
fn go_package_init_and_variables_run_when_the_package_loads() {
    let d = analyze(&[
        ("go.mod", "module svc\n\ngo 1.21\n"),
        ("main.go", "package main\n\nimport \"svc/cmd\"\n\nfunc main() {\n\tcmd.Execute()\n}\n"),
        ("cmd/root.go", "package cmd\n\nfunc Execute() {\n\trun()\n}\n"),
        ("cmd/serve.go", "package cmd\n\nimport \"svc/app\"\n\nvar serveCmd = func() error {\n\treturn app.Serve()\n}\n"),
        ("app/serve.go", "package app\n\nfunc Serve() error {\n\treturn nil\n}\n"),
    ]);
    let m = &d["flow"]["map"];
    assert!(m["edges"].as_array().unwrap().iter().all(|e| e.as_array().unwrap().len() == 3), "calls, not imports: {m}");
    let parts = strs(&m["parts"]);
    assert!(parts.iter().any(|u| u.starts_with("app")), "{m}");
}

#[test]
fn map_shows_a_part_that_is_imported_but_never_called() {
    let d = analyze(&[
        ("app/main.py", "import core.settings\n\ndef go():\n    return 1\n\nif __name__ == '__main__':\n    go()\n"),
        ("core/settings.py", "DEBUG = True\n"),
    ]);
    let m = &d["flow"]["map"];
    assert!(strs(&m["parts"]).contains(&"core".to_string()), "{m}");
    assert!(m["edges"].as_array().unwrap().iter().any(|e| e[0] == "app" && e[1] == "core" && e[3] == 1), "{m}");
    assert!(!strs(&d["flow"]["unreached"]).contains(&"core".to_string()));
}

#[test]
fn a_command_line_journey_follows_the_calls_in_order() {
    let d = analyze(&[
        ("cli.py", "import sys\nfrom app import core\n\ndef main():\n    args = sys.argv[1:]\n    data = core.load(args[0])\n    core.save(data)\n\nif __name__ == \"__main__\":\n    main()\n"),
        ("app/__init__.py", ""),
        ("app/core.py", "def load(p):\n    return open(p).read()\n\ndef save(d):\n    print(d)\n"),
    ]);
    let p = &procs(&d)[0];
    let s = steps(p);
    let got: Vec<(u64, &str)> = s.iter().map(|(d, _, n)| (*d, n.as_str())).collect();
    assert_eq!(got, [(0, "if __name__ == \"__main__\""), (1, "main()"), (2, "load()"), (2, "save()")]);
    let step_io = |i: usize| strs(&p["steps"][i]["io"]);
    assert_eq!((step_io(1), step_io(2), step_io(3)), (vec!["args".to_string()], vec!["file_in".to_string()], vec!["stdout".to_string()]));
    assert_eq!(strs(&p["io"]["file_in"]), ["app/core.py"]);
    assert!(p["narrative"].as_str().unwrap().starts_with("It starts at if __name__ == \"__main__\" in cli.py, which goes on to main()."), "{}", p["narrative"]);
}

#[test]
fn a_go_server_journey_and_its_requests() {
    let d = analyze(&[
        ("go.mod", "module example.com/svc\n"),
        ("main.go", "package main\n\nimport \"net/http\"\n\nfunc main() {\n\thttp.HandleFunc(\"/users\", listUsers)\n\tstart()\n}\n"),
        ("server.go", "package main\n\nimport \"net/http\"\n\nfunc start() {\n\thttp.ListenAndServe(\":8080\", nil)\n}\n"),
        ("handlers.go", "package main\n\nfunc listUsers(w http.ResponseWriter, r *http.Request) {\n\trows := query(\"SELECT id FROM users\")\n\t_ = rows\n}\n\nfunc query(q string) []string { return nil }\n"),
    ]);
    let p = &procs(&d)[0];
    let s = steps(p);
    let names: Vec<&str> = s.iter().map(|x| x.2.as_str()).collect();
    assert_eq!(names, ["main()", "listUsers()", "query()", "start()"]);
    assert_eq!(s[1].1, "ref"); // handed to HandleFunc, run for each request
    assert!(strs(&p["steps"][3]["io"]).contains(&"http_in".to_string()));
    let r = &p["routes"][0];
    assert_eq!((r["verb"].as_str(), r["path"].as_str()), (Some("ANY"), Some("/users")));
    assert_eq!(r["steps"][0]["n"], "listUsers()");
    assert!(strs(&r["io"]).contains(&"db".to_string()));
}

#[test]
fn an_express_request_ends_at_the_database() {
    let d = analyze(&[
        ("package.json", r#"{"name": "api", "main": "server.js", "dependencies": {"express": "4", "pg": "8"}}"#),
        ("server.js", "const express = require('express')\nconst { listUsers } = require('./users')\nconst app = express()\napp.get('/users', listUsers)\napp.listen(3000)\n"),
        ("users.js", "const db = require('./db')\nfunction listUsers(req, res) {\n  res.json(db.all())\n}\nmodule.exports = { listUsers }\n"),
        ("db.js", "const { Pool } = require('pg')\nconst pool = new Pool()\nfunction all() {\n  return pool.query('SELECT * FROM users')\n}\nmodule.exports = { all }\n"),
    ]);
    let p = &procs(&d)[0];
    assert!(strs(&p["steps"][0]["io"]).contains(&"http_in".to_string()));
    let r = &p["routes"][0];
    assert_eq!((r["verb"].as_str(), r["path"].as_str()), (Some("GET"), Some("/users")));
    let rs: Vec<&str> = r["steps"].as_array().unwrap().iter().map(|s| s["n"].as_str().unwrap()).collect();
    assert_eq!(rs, ["listUsers()", "all()"]);
    assert!(strs(&r["io"]).contains(&"db".to_string()));
}

#[test]
fn an_inline_handler_runs_per_request_and_a_router_module_at_start() {
    let d = analyze(&[
        ("package.json", r#"{"name": "api", "main": "server.js", "dependencies": {"express": "4"}}"#),
        ("server.js", "const express = require('express')\nconst routes = require('./routes')\nconst app = express()\napp.use(routes)\napp.listen(3000)\n"),
        ("routes.js", "const { Router } = require('express')\nconst { find } = require('./store')\nconst router = Router()\nrouter.get('/items', (req, res) => { res.json(find()) })\nmodule.exports = router\n"),
        ("store.js", "function find() { return [] }\nmodule.exports = { find }\n"),
    ]);
    let p = &procs(&d)[0];
    let names: Vec<String> = steps(p).into_iter().map(|x| x.2).collect();
    assert_eq!(names, ["server.js", "routes.js"]); // find() runs for each request, not at start
    let r = &p["routes"][0];
    assert_eq!((r["verb"].as_str(), r["path"].as_str()), (Some("GET"), Some("/items")));
    assert_eq!(r["steps"][0]["n"], "find()");
}

#[test]
fn a_call_on_an_object_of_several_classes_is_one_of_n() {
    let d = analyze(&[
        ("shapes.py", "class Shape:\n    def area(self):\n        return 0\n\nclass Square(Shape):\n    def area(self):\n        return 4\n\nclass Circle(Shape):\n    def area(self):\n        return 3\n"),
        ("main.py", "from shapes import Square, Circle\n\ndef total(items):\n    return sum(x.area() for x in items)\n\nif __name__ == \"__main__\":\n    print(total([Square(), Circle()]))\n"),
    ]);
    let s = steps(&procs(&d)[0]);
    let at = s.iter().position(|x| x.1 == "dispatch").expect("a dispatch step");
    assert!(s[at].2.starts_with("area() — one of 3"), "{:?}", s[at]);
    let alts: Vec<&str> = s[at + 1..at + 4].iter().map(|x| x.2.as_str()).collect();
    assert_eq!(sorted(alts.iter().map(|x| x.to_string()).collect()), ["Circle.area()", "Shape.area()", "Square.area()"]);
    assert!(s[at + 1..at + 4].iter().all(|x| x.0 == s[at].0 + 1));
}

#[test]
fn recursion_ends() {
    let d = analyze(&[("walk.py", "def walk(n):\n    if n:\n        walk(n - 1)\n\nif __name__ == \"__main__\":\n    walk(3)\n")]);
    let s = steps(&procs(&d)[0]);
    assert_eq!(s.len(), 3);
    assert_eq!(s[2].1, "recursion");
    assert_eq!(procs(&d)[0]["steps"][2]["seen"], 1);
}

#[test]
fn help_text_is_never_a_program() {
    // a string that mentions a file is not a command just because the file also runs another program
    let d = analyze(&[
        ("cli.py", "import argparse, subprocess\ndef main():\n    ap = argparse.ArgumentParser()\n    ap.add_argument(\"-o\", help=\"output file (default: map.html)\")\n    subprocess.run([\"git\", \"status\"])\nif __name__ == \"__main__\":\n    main()\n"),
        ("map.html", "<html></html>\n"),
    ]);
    assert_eq!(proc_names(&d), ["cli.py"]);
}

#[test]
fn dockerfile_entrypoint_script_and_server_command() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nWORKDIR /srv\nCOPY . /srv\nENTRYPOINT [\"/srv/entrypoint.sh\"]\n"),
        ("entrypoint.sh", "#!/bin/sh\nset -e\nexec gunicorn app.wsgi:application --bind 0.0.0.0:80\n"),
        ("app/__init__.py", ""),
        ("app/wsgi.py", "def application(environ, start_response):\n    return []\n"),
        ("requirements.txt", "gunicorn\n"),
    ]);
    let p = proc_named(&d, "gunicorn");
    assert_eq!(strs(&p["chain"]), ["entrypoint.sh"]);
    assert!(p["narrative"].as_str().unwrap().contains("Dockerfile runs entrypoint.sh, which runs `gunicorn app.wsgi:application`."), "{}", p["narrative"]);
    assert_eq!(p["steps"][0]["n"], "application()"); // the function the command names, not the file's top
}

// ---------------------------------------------------------------- the command

fn run(args: &[&str], cwd: Option<&Path>) -> std::process::Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_repoatlas"));
    c.args(args);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c.output().unwrap()
}

#[test]
fn cli_version() {
    let r = run(&["--version"], None);
    assert!(r.status.success());
    assert_eq!(String::from_utf8_lossy(&r.stdout).trim(), format!("repoatlas {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_maps_a_folder() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("app")).unwrap();
    fs::write(d.path().join("app/main.py"), "def hello():\n    pass\n").unwrap();
    let out = d.path().join("map.html");
    let r = run(&[d.path().join("app").to_str().unwrap(), "-o", out.to_str().unwrap(), "--no-open"], None);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let html = fs::read_to_string(out).unwrap();
    assert!(html.contains("architecture map"));
    assert!(html.contains("\"hello\""));
    assert!(!html.contains("__TITLE__"));
}

#[test]
fn cli_defaults_to_current_folder() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.py"), "x = 1\n").unwrap();
    let r = run(&["--no-open"], Some(d.path()));
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(d.path().join("repoatlas.html").exists());
}

#[test]
fn cli_errors_are_clear() {
    let r = run(&["/definitely/not/here", "--no-open"], None);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("is not a folder"));
    let d = tempfile::tempdir().unwrap();
    let r = run(&[d.path().to_str().unwrap(), "--no-open"], None);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("no source files"));
}

#[cfg(unix)]
#[test]
fn cli_never_reads_outside_the_repo_through_symlinks() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    fs::write(d.join("secret.txt"), "aws_secret_access_key = OUTSIDE-SECRET-VALUE in the credentials file\n").unwrap();
    fs::write(d.join("outside.py"), "\"\"\"OUTSIDE-DOC-VALUE from a file next to the repo.\"\"\"\n").unwrap();
    let repo = d.join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/app.py"), "def inside():\n    pass\n").unwrap();
    symlink("app.py", repo.join("src/same.py")).unwrap(); // inside the repo: still mapped
    symlink(d.join("secret.txt"), repo.join("README.md")).unwrap();
    symlink(d.join("outside.py"), repo.join("src/leak.py")).unwrap();
    let out = d.join("map.html");
    let r = run(&[repo.to_str().unwrap(), "-o", out.to_str().unwrap(), "--no-open"], None);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let html = fs::read_to_string(out).unwrap();
    assert!(html.contains("same.py"));
    assert!(!html.contains("leak.py"));
    assert!(!html.contains("OUTSIDE-SECRET-VALUE"));
    assert!(!html.contains("OUTSIDE-DOC-VALUE"));
}

// ---------------------------------------------------------------- exact call resolution

/// (file, name) of every step
fn step_files(p: &Value) -> Vec<(String, String)> {
    p["steps"].as_array().unwrap().iter().map(|s| (s["f"].as_str().unwrap().to_string(), s["n"].as_str().unwrap().to_string())).collect()
}

#[test]
fn rust_paths_follow_modules_and_reexports_and_skip_outside_crates() {
    let d = analyze(&[
        ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"),
        ("src/main.rs", "use std::fs;\n\nfn open() {}\n\nfn main() {\n    fs::File::open(\"x\");\n    std::fs::File::open(\"y\");\n    demo::analyze();\n}\n"),
        ("src/lib.rs", "mod atlas;\nmod parse;\npub use atlas::analyze;\n"),
        ("src/atlas.rs", "use crate::parse;\n\npub fn analyze() {\n    let v = vec![1];\n    v.iter().for_each(parse::parse);\n}\n"),
        ("src/parse.rs", "pub fn parse(x: &i32) {}\n"),
    ]);
    let p = &procs(&d)[0];
    let got = step_files(p);
    let names: Vec<&str> = got.iter().map(|x| x.1.as_str()).collect();
    assert_eq!(names, ["main()", "analyze()", "parse()"], "{got:?}"); // no open(): fs::File::open is the standard library's
    assert_eq!(got[1].0, "src/atlas.rs"); // through `pub use atlas::analyze` in lib.rs
    assert_eq!(got[2].0, "src/parse.rs"); // handed over as `parse::parse`
}

#[test]
fn python_named_imports_pick_the_exact_function_and_libraries_stay_outside() {
    let d = analyze(&[
        ("a/x.py", "def foo():\n    return 1\n"),
        ("b/x.py", "def foo():\n    return 2\n"),
        ("util.py", "def join():\n    return 3\n"),
        ("app.py", "import os\nfrom a.x import foo as bar\nfrom b import x\n\ndef run():\n    os.path.join('a')\n    bar()\n\nif __name__ == '__main__':\n    run()\n"),
    ]);
    let p = &procs(&d)[0];
    let got = step_files(p);
    assert!(got.contains(&("a/x.py".to_string(), "foo()".to_string())), "{got:?}");
    assert!(!got.iter().any(|g| g.0 == "b/x.py" || g.1 == "join()"), "{got:?}");
}

#[test]
fn a_call_matched_only_by_the_object_name_is_marked_as_a_guess() {
    let d = analyze(&[
        ("app.py", "from store import make\n\ndef run(thing):\n    thing.persist()\n\nif __name__ == '__main__':\n    run(make())\n"),
        ("store.py", "class Box:\n    def persist(self):\n        pass\n\ndef make():\n    return Box()\n"),
    ]);
    let p = &procs(&d)[0];
    let steps = p["steps"].as_array().unwrap();
    let persist = steps.iter().find(|s| s["n"] == "Box.persist()").expect("a step for Box.persist");
    assert_eq!(persist["guess"], true);
    let make = steps.iter().find(|s| s["n"] == "make()").expect("a step for make");
    assert!(make.get("guess").is_none());
}

#[test]
fn what_a_route_hands_over_before_its_handler_is_middleware() {
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"app.js\",\"dependencies\":{\"express\":\"4\"}}"),
        ("app.js", "const express = require('express');\nconst auth = require('./auth');\nconst svc = require('./svc');\nconst app = express();\napp.get('/items', auth.required, (req, res) => { res.json(svc.list()); });\napp.listen(3000);\n"),
        ("auth.js", "module.exports = { required: (req, res, next) => next() };\n"),
        ("svc.js", "function list() { return []; }\nmodule.exports = { list };\n"),
    ]);
    let r = &procs(&d)[0]["routes"][0];
    let s = steps(r);
    assert_eq!((s[0].1.as_str(), s[0].2.as_str()), ("middleware", "required()"), "{s:?}");
    assert_eq!(s[1].2, "list()", "{s:?}");
    assert_eq!(s.len(), 2, "{s:?}");
}

#[test]
fn a_declared_type_decides_which_class_a_method_belongs_to() {
    // two classes have `process`: the parameter's type says which one runs
    let d = analyze(&[
        ("app.py", "from jobs import Worker\n\ndef handle(w: Worker):\n    w.process()\n\nif __name__ == '__main__':\n    handle(Worker())\n"),
        ("jobs.py", "class Worker:\n    def process(self):\n        pass\n\nclass Other:\n    def process(self):\n        pass\n"),
    ]);
    let got = steps(&procs(&d)[0]);
    let names: Vec<&str> = got.iter().map(|s| s.2.as_str()).collect();
    assert!(names.contains(&"Worker.process()"), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("Other") || n.contains("one of")), "{names:?}");
}

#[test]
fn a_constructor_injected_field_gives_its_class() {
    let d = analyze(&[
        ("main.ts", "import { Ctl } from './ctl';\nimport { Service } from './svc';\n\nconst c = new Ctl(new Service());\nc.go();\n"),
        ("ctl.ts", "import { Service } from './svc';\n\nexport class Ctl {\n  constructor(private svc: Service) {}\n  go() { this.svc.runJob(); }\n}\n"),
        ("svc.ts", "export class Service {\n  runJob() {}\n}\nexport class Other {\n  runJob() {}\n}\n"),
    ]);
    let got = steps(&procs(&d)[0]);
    let names: Vec<&str> = got.iter().map(|s| s.2.as_str()).collect();
    assert!(names.contains(&"Service.runJob()"), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("Other") || n.contains("one of")), "{names:?}");
}

#[test]
fn a_step_says_when_its_call_runs() {
    let d = analyze(&[
        ("app.py", "from util import a, b, c, d\n\ndef run(x):\n    d()\n    if x:\n        a()\n    for i in x:\n        b()\n    try:\n        pass\n    except Exception:\n        c()\n\nif __name__ == '__main__':\n    run(1)\n"),
        ("util.py", "def a(): pass\ndef b(): pass\ndef c(): pass\ndef d(): pass\n"),
    ]);
    let p = &procs(&d)[0];
    let when = |n: &str| p["steps"].as_array().unwrap().iter().find(|s| s["n"] == n).map(|s| s["when"].as_str().unwrap_or("").to_string()).unwrap();
    assert_eq!((when("a()"), when("b()"), when("c()"), when("d()")), ("if".into(), "loop".into(), "error".into(), "".into()));
}

// ---------------------------------------------------------------- route prefixes

fn route_paths(p: &Value) -> Vec<String> {
    sorted(p["routes"].as_array().unwrap().iter().map(|r| format!("{} {}", r["verb"].as_str().unwrap(), r["path"].as_str().unwrap())).collect())
}

#[test]
fn a_routers_prefixes_are_joined_with_the_constant_they_name() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"app.main:app\"]\n"),
        ("app/__init__.py", ""),
        ("app/config.py", "class Settings:\n    API_V1_STR: str = \"/api/v1\"\n\nsettings = Settings()\n"),
        ("app/main.py", "from fastapi import FastAPI\nfrom app.config import settings\nfrom app.api.main import api_router\n\napp = FastAPI()\napp.include_router(api_router, prefix=settings.API_V1_STR)\n"),
        ("app/api/__init__.py", ""),
        ("app/api/main.py", "from fastapi import APIRouter\nfrom app.api.routes import items\n\napi_router = APIRouter()\napi_router.include_router(items.router)\n"),
        ("app/api/routes/__init__.py", ""),
        ("app/api/routes/items.py", "from fastapi import APIRouter\n\nrouter = APIRouter(prefix=\"/items\", tags=[\"items\"])\n\n@router.get(\"/\")\ndef read_items():\n    return []\n\n@router.get(\"/{id}\")\ndef read_item(id):\n    return id\n"),
    ]);
    let p = &procs(&d)[0];
    assert_eq!(route_paths(p), ["GET /api/v1/items/", "GET /api/v1/items/{id}"]);
}

#[test]
fn an_unknown_prefix_constant_is_shown_by_name() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"app:app\"]\n"),
        ("app.py", "from fastapi import FastAPI\nfrom items import router\n\napp = FastAPI()\napp.include_router(router, prefix=BASE)\n"),
        ("items.py", "from fastapi import APIRouter\nrouter = APIRouter()\n\n@router.get(\"/x\")\ndef x():\n    return 1\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /{BASE}/x"]);
}

#[test]
fn express_mounts_put_their_path_in_front() {
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"main.js\",\"dependencies\":{\"express\":\"4\"}}"),
        ("main.js", "const express = require('express');\nconst routes = require('./routes');\nconst app = express();\napp.use('/api', routes);\napp.listen(3000);\n"),
        ("routes.js", "const router = require('express').Router();\nconst users = require('./users');\nrouter.use('/users', users);\nmodule.exports = router;\n"),
        ("users.js", "const router = require('express').Router();\nrouter.get('/:id', (req, res) => { res.json({}); });\nmodule.exports = router;\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /api/users/:id"]);
}

#[test]
fn a_controller_class_prefix_comes_before_its_methods() {
    let d = analyze(&[
        ("src/main/java/app/Main.java", "package app;\n\npublic class Main {\n    public static void main(String[] a) {}\n}\n"),
        ("src/main/java/app/UserController.java", "package app;\n\n@RestController\n@RequestMapping(\"/users\")\npublic class UserController {\n    @GetMapping(\"/{id}\")\n    public String get() { return \"\"; }\n}\n"),
    ]);
    let all: Vec<String> = procs(&d).iter().flat_map(route_paths).collect();
    assert!(all.contains(&"GET /users/{id}".to_string()), "{all:?}");
}

#[test]
fn a_chained_router_takes_the_name_it_is_given() {
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"main.js\",\"dependencies\":{\"express\":\"4\"}}"),
        ("main.js", "const express = require('express');\nconst routes = require('./routes');\nconst app = express();\napp.use(routes);\napp.listen(3000);\n"),
        ("routes.js", "const { Router } = require('express');\nconst tags = require('./tags');\nconst api = Router()\n  .use(tags);\n\nmodule.exports = Router().use('/api', api);\n"),
        ("tags.js", "const router = require('express').Router();\nrouter.get('/tags', (req, res) => { res.json([]); });\nmodule.exports = router;\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /api/tags"]);
}

#[test]
fn a_signature_alias_runs_the_functions_it_hands_over() {
    let d = analyze(&[
        ("deps.py", "from typing import Annotated\nfrom fastapi import Depends\n\ndef get_db():\n    return 1\n\nSessionDep = Annotated[object, Depends(get_db)]\n"),
        ("routes.py", "from deps import SessionDep\n\ndef read(session: SessionDep):\n    return 1\n"),
        ("main.py", "from routes import read\n\nif __name__ == '__main__':\n    read()\n"),
    ]);
    let got = steps(&procs(&d)[0]);
    let names: Vec<&str> = got.iter().map(|s| s.2.as_str()).collect();
    assert_eq!(names, ["if __name__ == '__main__'", "read()", "get_db()"], "{got:?}");
    assert_eq!(got[2].0, 2); // under read()
}

// ---------------------------------------------------------------- SCIP index

fn occ(range: &[i32], symbol: &str, role: i32) -> scip::types::Occurrence {
    scip::types::Occurrence { range: range.to_vec(), symbol: symbol.into(), symbol_roles: role, ..Default::default() }
}

fn scip_repo(files: &[(&str, &str)], docs: Vec<(&str, Vec<scip::types::Occurrence>)>) -> Value {
    use protobuf::Message;
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        fs::write(d.path().join(rel), text).unwrap();
    }
    let index = scip::types::Index {
        documents: docs.into_iter().map(|(p, o)| scip::types::Document { relative_path: p.into(), occurrences: o, ..Default::default() }).collect(),
        ..Default::default()
    };
    fs::write(d.path().join("index.scip"), index.write_to_bytes().unwrap()).unwrap();
    repoatlas::analyze(d.path(), "fixture").unwrap()
}

#[test]
fn a_scip_index_settles_which_class_a_call_runs_and_what_is_outside() {
    let app = "from m import A, B\n\ndef run(x):\n    x.process()\n    x.helper()\n\nif __name__ == '__main__':\n    run(A())\n";
    let m = "class A:\n    def process(self):\n        pass\n    def helper(self):\n        pass\n\nclass B:\n    def process(self):\n        pass\n";
    // without the index both classes have `process`, and `helper` is guessed from the one class that has it
    let plain = analyze(&[("app.py", app), ("m.py", m)]);
    let names = |d: &Value| steps(&procs(d)[0]).into_iter().map(|s| s.2).collect::<Vec<_>>();
    let before = names(&plain);
    assert!(before.contains(&"A.helper()".to_string()) && before.iter().any(|n| n.contains("one of")), "{before:?}");

    let d = scip_repo(&[("app.py", app), ("m.py", m)], vec![
        ("app.py", vec![occ(&[3, 6, 13], "scip-python python demo 1 m/B#process().", 0), occ(&[4, 6, 12], "scip-python python lib 1 lib/Thing#helper().", 0)]),
        ("m.py", vec![
            occ(&[1, 8, 15], "scip-python python demo 1 m/A#process().", 1),
            occ(&[3, 8, 14], "scip-python python demo 1 m/A#helper().", 1),
            occ(&[7, 8, 15], "scip-python python demo 1 m/B#process().", 1),
        ]),
    ]);
    let after = names(&d);
    assert!(after.contains(&"B.process()".to_string()), "{after:?}");
    assert!(!after.iter().any(|n| n == "A.process()" || n == "A.helper()" || n.contains("one of")), "{after:?}");
    assert_eq!(d["repo"]["scip"]["calls"], 2);
}

#[test]
fn a_call_on_an_interface_runs_what_the_index_says_implements_it() {
    let app = "from m import Shape\n\ndef show(s: Shape):\n    s.area()\n\nif __name__ == '__main__':\n    show(None)\n";
    let m = "class Shape:\n    def area(self):\n        pass\n\nclass Square(Shape):\n    def area(self):\n        return 4\n\nclass Circle(Shape):\n    def area(self):\n        return 3\n";
    let mut square = scip::types::SymbolInformation { symbol: "py m/Square#area().".into(), ..Default::default() };
    square.relationships.push(scip::types::Relationship { symbol: "py m/Shape#area().".into(), is_implementation: true, ..Default::default() });
    use protobuf::Message;
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("app.py"), app).unwrap();
    fs::write(dir.path().join("m.py"), m).unwrap();
    let index = scip::types::Index {
        documents: vec![
            scip::types::Document { relative_path: "app.py".into(), occurrences: vec![occ(&[3, 6, 10], "py m/Shape#area().", 0)], ..Default::default() },
            scip::types::Document {
                relative_path: "m.py".into(),
                occurrences: vec![occ(&[1, 8, 12], "py m/Shape#area().", 1), occ(&[5, 8, 12], "py m/Square#area().", 1)],
                symbols: vec![square],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    fs::write(dir.path().join("index.scip"), index.write_to_bytes().unwrap()).unwrap();
    let d = repoatlas::analyze(dir.path(), "fixture").unwrap();
    let names: Vec<String> = steps(&procs(&d)[0]).into_iter().map(|s| s.2).collect();
    // the interface's method and the one implementation the index lists; Circle is not named by it
    assert!(names.contains(&"Shape.area()".to_string()) && names.contains(&"Square.area()".to_string()), "{names:?}");
    assert!(!names.contains(&"Circle.area()".to_string()), "{names:?}");
}

#[test]
fn a_bad_scip_file_given_on_the_command_line_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.py"), "def f(): pass\n").unwrap();
    fs::write(d.path().join("bad.scip"), b"\xff\xfe not protobuf").unwrap();
    let err = repoatlas::analyze_with(d.path(), "x", Some(&d.path().join("bad.scip"))).unwrap_err();
    assert!(err.contains("bad.scip"), "{err}");
    // an index.scip found by itself that cannot be read is ignored
    fs::write(d.path().join("index.scip"), b"\xff\xfe not protobuf").unwrap();
    assert!(repoatlas::analyze(d.path(), "x").is_ok());
}

#[test]
fn rule_move_changes_nothing() {
    // pins what the route tables produce today, so moving them into rules.rs cannot change a journey
    let py = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"),
        ("main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get(\"/ping\")\ndef ping():\n    return 1\n\n@app.route(\"/old\", methods=[\"GET\", \"POST\"])\ndef old():\n    return 2\n"),
    ]);
    let js = analyze(&[
        ("server.js", "const express = require('express')\nconst app = express()\napp.get('/users', list)\napp.post('/users', (req, res) => res.send(make()))\nfunction list() {}\nfunction make() {}\napp.listen(3000)\n"),
    ]);
    let go = analyze(&[
        ("main.go", "package main\n\nimport \"net/http\"\n\nfunc main() {\n\tmux := http.NewServeMux()\n\tmux.HandleFunc(\"/health\", health)\n\thttp.ListenAndServe(\":80\", mux)\n}\n\nfunc health(w http.ResponseWriter, r *http.Request) {}\n"),
    ]);
    let all: Vec<Vec<String>> = [&py, &js, &go].iter().map(|d| route_paths(&procs(d)[0])).collect();
    assert_eq!(all, [vec!["GET /ping", "GET|POST /old"], vec!["GET /users", "POST /users"], vec!["ANY /health"]]);
}

// ---------------------------------------------------------------- route handlers

/// the name of the first step of the journey of the route whose path is `path`
fn route_first(d: &Value, path: &str) -> Option<String> {
    let r = procs(d).iter().flat_map(|p| p["routes"].as_array().cloned().unwrap_or_default()).find(|r| r["path"] == path)?;
    r["steps"].as_array()?.first().map(|s| s["n"].as_str().unwrap().to_string())
}

#[test]
fn go_122_patterns() {
    let d = analyze(&[(
        "main.go",
        "package main\n\nimport \"net/http\"\n\nfunc main() {\n\tmux := http.NewServeMux()\n\tmux.HandleFunc(\"GET /users/{id}\", getUser)\n\thttp.ListenAndServe(\":80\", mux)\n}\n\nfunc getUser(w http.ResponseWriter, r *http.Request) {}\n",
    )]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /users/{id}"]);
    assert_eq!(route_first(&d, "/users/{id}").as_deref(), Some("getUser()"));
}

#[test]
fn handler_forms() {
    let axum = analyze(&[(
        "src/main.rs",
        "use axum::{routing::get, Router};\n\nfn main() {\n    let app = Router::new().route(\"/items\", get(list_items));\n    axum::serve(listener, app);\n}\n\nfn list_items() {}\n",
    )]);
    assert_eq!(route_first(&axum, "/items").as_deref(), Some("list_items()"));
    let laravel = analyze(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../routes/web.php';\n"),
        ("routes/web.php", "<?php\nRoute::get('/users', [UserController::class, 'index']);\nRoute::post('/login', 'AuthController@login');\n"),
        ("app/Http/Controllers/UserController.php", "<?php\nclass UserController {\n    public function index() {}\n}\n"),
        ("app/Http/Controllers/AuthController.php", "<?php\nclass AuthController {\n    public function login() {}\n}\n"),
    ]);
    assert_eq!(route_first(&laravel, "/users").as_deref(), Some("UserController.index()"));
    assert_eq!(route_first(&laravel, "/login").as_deref(), Some("AuthController.login()"));
    let js = analyze(&[
        ("server.js", "const express = require('express')\nconst ctrl = require('./ctrl')\nconst app = express()\napp.get('/p', ctrl.profile)\napp.listen(3000)\n"),
        ("ctrl.js", "exports.profile = function profile() {}\n"),
    ]);
    assert_eq!(route_first(&js, "/p").as_deref(), Some("profile()"));
}

#[test]
fn unknown_handler_is_harmless() {
    let d = analyze(&[("server.js", "const express = require('express')\nconst app = express()\napp.get('/x', missing)\napp.listen(3000)\n")]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /x"]);
    assert_eq!(route_first(&d, "/x"), None);
}

// ---------------------------------------------------------------- attribute routes and routes files

#[test]
fn rust_attribute_routes() {
    let d = analyze(&[(
        "src/main.rs",
        "use actix_web::{get, route, App, HttpServer};\n\n#[get(\"/users\")]\nasync fn list() {}\n\n#[route(\"/x\", method = \"GET\")]\nasync fn x() {}\n\nfn main() {\n    HttpServer::new(|| App::new().service(list).service(x));\n}\n",
    )]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /users", "GET /x"]);
}

#[test]
fn php_attribute_routes() {
    let d = analyze(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../src/A.php';\n"),
        ("src/A.php", "<?php\n#[Route('/api')]\nclass A {\n    #[Route('/users', methods: ['GET'])]\n    public function list() {}\n}\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /api/users"]);
}

#[test]
fn routes_file_without_leading_slash() {
    let d = analyze(&[
        ("main.py", "from app import urls, util\n\nif __name__ == '__main__':\n    util.run()\n"),
        ("app/__init__.py", ""),
        ("app/urls.py", "from django.urls import path\nfrom app import views\n\nurlpatterns = [path(\"users/\", views.users)]\n"),
        ("app/views.py", "def users():\n    return 1\n"),
        ("app/util.py", "def run():\n    return path(\"users/\", 1)\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["ANY /users/"]); // util.py is not a routes file: path() there is just a call
}

#[test]
fn a_call_named_like_a_route_outside_a_routes_file_is_not_one() {
    let d = analyze(&[
        ("routes/web.php", "<?php\nRoute::get('users', 'U@index');\n"),
        ("server.js", "const express = require('express')\nconst app = express()\nconst name = cache.get('name')\napp.listen(3000)\n"),
        ("util.py", "def f(logger):\n    return logger.get(\"name\")\n"),
    ]);
    let all: Vec<String> = d["nodes"].as_object().unwrap().values().flat_map(|n| n["symbols"].as_array().cloned().unwrap_or_default()).filter(|s| s["k"] == "route").map(|s| s["n"].as_str().unwrap().to_string()).collect();
    assert_eq!(all, ["users"]);
}

// ---------------------------------------------------------------- includes and closure groups

#[test]
fn django_include() {
    let d = analyze(&[
        ("main.py", "from config import urls\nfrom shop import urls as shop_urls\n\nif __name__ == '__main__':\n    urls.x\n"),
        ("config/__init__.py", ""),
        ("config/urls.py", "from django.urls import path, include\n\nurlpatterns = [path(\"api/\", include(\"shop.urls\"))]\n"),
        ("shop/__init__.py", ""),
        ("shop/urls.py", "from django.urls import path\nfrom shop import views\n\nurlpatterns = [path(\"items/\", views.items)]\n"),
        ("shop/views.py", "def items():\n    return 1\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["ANY /api/items/"]); // the include itself is not a route
}

#[test]
fn laravel_group() {
    let d = analyze(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../routes/web.php';\n"),
        ("routes/web.php", "<?php\nRoute::prefix('api')->group(function () {\n    Route::get('/users', 'U@index');\n});\nRoute::group(['prefix' => 'admin'], function () {\n    Route::get('/stats', 'U@stats');\n});\nRoute::get('/home', 'U@home');\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /admin/stats", "GET /api/users", "GET /home"]);
}

#[test]
fn chi_route_group() {
    let d = analyze(&[(
        "main.go",
        "package main\n\nimport \"net/http\"\n\nfunc main() {\n\tr := chi.NewRouter()\n\tr.Route(\"/v1\", func(r chi.Router) {\n\t\tr.Get(\"/x\", h)\n\t})\n\tr.Get(\"/top\", h)\n\thttp.ListenAndServe(\":80\", r)\n}\n\nfunc h(w http.ResponseWriter, r *http.Request) {}\n",
    )]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /top", "GET /v1/x"]);
}

#[test]
fn same_router_name_two_files() {
    let d = analyze(&[
        ("main.js", "const express = require('express')\nconst one = require('./one')\nconst two = require('./two')\nconst app = express()\napp.use('/one', one)\napp.use('/two', two)\napp.listen(3000)\n"),
        ("one.js", "const { Router } = require('express')\nconst router = Router()\nrouter.get('/x', (req, res) => res.send(1))\nmodule.exports = router\n"),
        ("two.js", "const { Router } = require('express')\nconst router = Router()\nrouter.get('/y', (req, res) => res.send(2))\nmodule.exports = router\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /one/x", "GET /two/y"]);
}

// ---------------------------------------------------------------- php namespaces

#[test]
fn php_use_follows_psr4() {
    let d = analyze(&[
        ("composer.json", r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#),
        ("app/Http/Controllers/UserController.php", "<?php\nnamespace App\\Http\\Controllers;\n\nuse App\\Models\\User;\n\nclass UserController {\n    public function index() { return User::all(); }\n}\n"),
        ("app/Models/User.php", "<?php\nnamespace App\\Models;\n\nclass User {}\n"),
        ("app/Legacy/User.php", "<?php\nnamespace App\\Legacy;\n\nclass User {}\n"),
    ]);
    // two classes are called User: the `use` line says which one
    assert_eq!(uses(&d, "app/Http/Controllers/UserController.php"), ["app/Models/User.php"]);
}

// ---------------------------------------------------------------- orm calls

/// the "db" labels of every step of the journey of the route `path` (or of the program when `path` is empty), flattened
fn db_steps(d: &Value, path: &str) -> Vec<String> {
    let p = &procs(d)[0];
    let steps: Vec<Value> = if path.is_empty() {
        p["steps"].as_array().cloned().unwrap_or_default()
    } else {
        p["routes"].as_array().unwrap().iter().filter(|r| r["path"] == path).flat_map(|r| r["steps"].as_array().cloned().unwrap_or_default()).collect()
    };
    steps.iter().flat_map(|s| strs(&s["db"])).collect()
}

#[test]
fn django_orm() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"),
        ("shop/models.py", "from django.db import models\n\nclass User(models.Model):\n    name = models.CharField()\n"),
        ("main.py", "from fastapi import FastAPI\nfrom shop.models import User\n\napp = FastAPI()\n\n@app.get(\"/profile\")\ndef profile():\n    return User.objects.filter(id=1).first()\n\n@app.post(\"/rename\")\ndef rename():\n    user = User.objects.get(id=1)\n    user.name = \"x\"\n    user.save()\n"),
    ]);
    assert_eq!(db_steps(&d, "/profile"), ["reads User"]);
    assert_eq!(db_steps(&d, "/rename"), ["reads User", "writes User"]);
}

#[test]
fn plain_collections_are_not_the_database() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"),
        ("main.py", "from fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get(\"/x\")\ndef x(d, items, s, q):\n    a = d.get(\"a\")\n    b = items.filter(lambda i: i)\n    s.add(1)\n    q.put(2)\n    return a\n"),
    ]);
    assert_eq!(db_steps(&d, "/x"), Vec::<String>::new());
    assert!(!d["flow"]["processes"][0]["routes"][0]["io"].as_array().unwrap().iter().any(|k| k.as_str().unwrap().starts_with("db")));
}

#[test]
fn an_imported_model_that_is_not_used_adds_no_step() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"),
        ("shop/models.py", "from django.db import models\n\nclass User(models.Model):\n    name = models.CharField()\n"),
        ("main.py", "from fastapi import FastAPI\nfrom shop.models import User\n\napp = FastAPI()\n\n@app.get(\"/ping\")\ndef ping():\n    return 1\n"),
    ]);
    assert_eq!(db_steps(&d, "/ping"), Vec::<String>::new());
}

#[test]
fn sqlalchemy_session() {
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"),
        ("models.py", "from sqlalchemy.orm import DeclarativeBase\n\nclass Base(DeclarativeBase):\n    pass\n\nclass User(Base):\n    pass\n"),
        ("main.py", "from fastapi import FastAPI\nfrom sqlalchemy import select\nfrom sqlalchemy.orm import Session\nfrom models import User\n\napp = FastAPI()\n\n@app.get(\"/users\")\ndef list_users(db: Session):\n    return db.execute(select(User).where(User.id > 1)).scalars().all()\n\n@app.post(\"/users\")\ndef add_user(db: Session, u):\n    db.add(u)\n    db.commit()\n"),
    ]);
    let r = |verb: &str| procs(&d)[0]["routes"].as_array().unwrap().iter().find(|r| r["verb"] == verb).unwrap().clone();
    let labels = |r: Value| r["steps"].as_array().unwrap().iter().flat_map(|s| strs(&s["db"])).collect::<Vec<_>>();
    assert_eq!(labels(r("GET")), ["reads User"]);
    assert_eq!(labels(r("POST")), ["writes database"]); // the model is not named in the call
}

// ---------------------------------------------------------------- tier 1 frameworks and ORMs

fn one_route_set(files: &[(&str, &str)]) -> (Value, Vec<String>) {
    let d = analyze(files);
    let r = route_paths(&procs(&d)[0]);
    (d, r)
}

#[test]
fn python_frameworks() {
    // Flask blueprint with a url_prefix, mounted on the app
    let (_, r) = one_route_set(&[
        ("main.py", "from flask import Flask\nfrom api import bp\n\napp = Flask(__name__)\napp.register_blueprint(bp)\n\nif __name__ == \"__main__\":\n    app.run()\n"),
        ("api.py", "from flask import Blueprint\n\nbp = Blueprint(\"api\", __name__, url_prefix=\"/api\")\n\n@bp.route(\"/users\")\ndef users():\n    return []\n"),
    ]);
    assert_eq!(r, ["ANY /api/users"]);
    // Litestar: a bare decorator
    let (_, r) = one_route_set(&[("main.py", "from litestar import Litestar, get\n\n@get(\"/users\")\nasync def users() -> list:\n    return []\n\napp = Litestar([users])\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(r, ["GET /users"]);
    // Starlette: a Route object in a list
    let (d, r) = one_route_set(&[("main.py", "from starlette.applications import Starlette\nfrom starlette.routing import Route\n\nasync def users(request):\n    return 1\n\napp = Starlette(routes=[Route(\"/users\", users)])\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(r, ["ANY /users"]);
    assert_eq!(route_first(&d, "/users").as_deref(), Some("users()"));
    // Flask: a view function handed over by keyword
    let (d, r) = one_route_set(&[("main.py", "from flask import Flask\n\napp = Flask(__name__)\n\ndef users():\n    return 1\n\napp.add_url_rule(\"/users\", view_func=users)\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(r, ["ANY /users"]);
    assert_eq!(route_first(&d, "/users").as_deref(), Some("users()"));
    // Django REST framework: a viewset registered on a router is five endpoints
    let (d, r) = one_route_set(&[
        ("main.py", "import urls\nimport views\n\nif __name__ == \"__main__\":\n    urls.router\n"),
        ("urls.py", "from rest_framework.routers import DefaultRouter\nfrom views import UserViewSet\n\nrouter = DefaultRouter()\nrouter.register(r\"users\", UserViewSet)\nurlpatterns = router.urls\n"),
        ("views.py", "from rest_framework import viewsets\n\nclass UserViewSet(viewsets.ModelViewSet):\n    def list(self, request):\n        return 1\n"),
    ]);
    assert_eq!(r, ["DELETE /users/{pk}/", "GET /users/", "GET /users/{pk}/", "POST /users/", "PUT /users/{pk}/"]);
    assert_eq!(route_first(&d, "/users/").as_deref(), Some("UserViewSet.list()"));
}

#[test]
fn python_orms() {
    let routes = |models: &[(&str, &str)], body: &str| {
        let mut files: Vec<(&str, &str)> = vec![("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n")];
        files.extend_from_slice(models);
        let main = format!("from fastapi import FastAPI\nfrom models import User\n\napp = FastAPI()\n\n@app.get(\"/x\")\nasync def x():\n{body}\n");
        let mut all = files.clone();
        all.push(("main.py", main.as_str()));
        db_steps(&analyze(&all), "/x")
    };
    // Peewee
    let got = routes(&[("models.py", "import peewee\n\nclass User(peewee.Model):\n    name = peewee.CharField()\n")], "    User.create(name='x')\n    return User.select().where(User.id == 1).get()");
    assert_eq!(got, ["writes User", "reads User"]);
    // Tortoise
    let got = routes(&[("models.py", "from tortoise.models import Model\n\nclass User(Model):\n    pass\n")], "    await User.create(name='x')\n    return await User.filter(id=1).first()");
    assert_eq!(got, ["writes User", "reads User"]);
    // SQLModel: a model, and a session handed in
    let got = routes(&[("models.py", "from sqlmodel import SQLModel\n\nclass User(SQLModel, table=True):\n    pass\n")], "    return User.model_validate({})");
    assert_eq!(got, Vec::<String>::new()); // SQLModel has no query methods on the class: no invented step
}

/// every route of the first program: "VERB /path first-steps" (the steps a request starts at, joined by ", "), sorted
fn route_lines(d: &Value) -> Vec<String> {
    sorted(procs(d)[0]["routes"].as_array().unwrap().iter().map(|r| {
        let starts: Vec<&str> = r["steps"].as_array().unwrap().iter().filter(|s| s["d"] == 0).map(|s| s["n"].as_str().unwrap()).collect();
        format!("{} {} {}", r["verb"].as_str().unwrap(), r["path"].as_str().unwrap(), starts.join(", "))
    }).collect())
}

/// whether the first step of the route `path` is marked as a guess
fn route_guess(d: &Value, path: &str) -> bool {
    let r = procs(d)[0]["routes"].as_array().unwrap().iter().find(|r| r["path"] == path).cloned().unwrap();
    r["steps"][0]["guess"] == true
}

fn py_routes(src: &str) -> Value {
    analyze(&[("main.py", src)])
}

#[test]
fn python_falcon_routes() {
    // Falcon: a resource object on a path answers with its on_<verb> methods
    let d = py_routes("import falcon\n\nclass UserResource:\n    def on_get(self, req, resp):\n        resp.media = []\n\n    def on_post(self, req, resp):\n        resp.status = 201\n\n    def helper(self):\n        return 1\n\napp = falcon.App()\napp.add_route(\"/users\", UserResource())\n\nif __name__ == \"__main__\":\n    app.run()\n");
    assert_eq!(route_lines(&d), ["GET /users UserResource.on_get()", "POST /users UserResource.on_post()"]);
}

#[test]
fn python_pyramid_routes() {
    // Pyramid: a named route, and the view that names it; the pairing is read off the name, so it is a guess
    let d = py_routes("from pyramid.config import Configurator\nfrom pyramid.view import view_config\n\n@view_config(route_name=\"users\", request_method=\"GET\")\ndef list_users(request):\n    return []\n\n@view_config(route_name=\"user\", request_method=\"POST\", renderer=\"json\")\ndef add_user(request):\n    return {}\n\ndef main():\n    with Configurator() as config:\n        config.add_route(\"users\", \"/users\")\n        config.add_route(\"user\", \"/users/{id}\")\n        config.scan()\n        return config.make_wsgi_app()\n\nif __name__ == \"__main__\":\n    main()\n");
    assert_eq!(route_lines(&d), ["GET /users list_users()", "POST /users/{id} add_user()"]);
    assert!(route_guess(&d, "/users") && route_guess(&d, "/users/{id}"));
}

#[test]
fn python_tornado_routes() {
    // Tornado: (path, handler class) pairs in the application's list; the handler answers with its verb methods
    let d = py_routes("import tornado.ioloop\nimport tornado.web\n\nclass MainHandler(tornado.web.RequestHandler):\n    def get(self):\n        self.write(\"hi\")\n\nclass UserHandler(tornado.web.RequestHandler):\n    def get(self, id):\n        self.write(id)\n\n    def post(self, id):\n        self.write(id)\n\ndef make_app():\n    return tornado.web.Application([\n        (r\"/\", MainHandler),\n        (r\"/users/(?P<id>[0-9]+)\", UserHandler),\n    ])\n\nif __name__ == \"__main__\":\n    app = make_app()\n    app.listen(8888)\n    tornado.ioloop.IOLoop.current().start()\n");
    assert_eq!(route_lines(&d), ["GET / MainHandler.get()", "GET /users/{id} UserHandler.get()", "POST /users/{id} UserHandler.post()"]);
}

#[test]
fn python_bottle_routes() {
    // Bottle: bare decorators, and a route on an app object with its method
    let d = py_routes("from bottle import Bottle, get, route, run\n\napp = Bottle()\n\n@route(\"/hello\")\ndef hello():\n    return \"hi\"\n\n@get(\"/users\")\ndef users():\n    return []\n\n@app.route(\"/items\", method=\"POST\")\ndef add_item():\n    return {}\n\nif __name__ == \"__main__\":\n    run(app)\n");
    assert_eq!(route_lines(&d), ["ANY /hello hello()", "GET /users users()", "POST /items add_item()"]);
}

#[test]
fn python_quart_routes() {
    // Quart: Flask-style
    let d = py_routes("from quart import Quart\n\napp = Quart(__name__)\n\n@app.route(\"/users\")\nasync def users():\n    return []\n\n@app.post(\"/users\")\nasync def add_user():\n    return {}\n\nif __name__ == \"__main__\":\n    app.run()\n");
    assert_eq!(route_lines(&d), ["ANY /users users()", "POST /users add_user()"]);
}

#[test]
fn python_django_ninja_routes() {
    // Django Ninja: verbs on the API and on a router mounted under a prefix
    let d = py_routes("from ninja import NinjaAPI, Router\n\napi = NinjaAPI()\nrouter = Router()\n\n@api.get(\"/users\")\ndef list_users(request):\n    return []\n\n@router.post(\"/items\")\ndef add_item(request):\n    return {}\n\napi.add_router(\"/shop/\", router)\n\nif __name__ == \"__main__\":\n    run(api)\n");
    assert_eq!(route_lines(&d), ["GET /users list_users()", "POST /shop/items add_item()"]);
}

#[test]
fn python_route_data_does_not_reach_other_languages() {
    // Go and JS keep their class-less routes: a call that builds a value is not a resource handed over
    let go = analyze(&[("main.go", "package main\n\nimport \"net/http\"\n\nfunc main() {\n\thttp.Handle(\"/users\", newHandler())\n\thttp.ListenAndServe(\":80\", nil)\n}\n\nfunc newHandler() http.Handler { return nil }\n")]);
    assert_eq!(route_lines(&go), ["ANY /users newHandler()"]);
    // JS: a call named like a named-route call, or a list of pairs handed to an `Application`, registers nothing outside Python
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"main.js\"}"),
        ("main.js", "const app = new Application([['/a', A]])\napp.add_route('users', '/users')\napp.get('/b', (req, res) => res.send(1))\napp.listen(3000)\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /b"]);
    // Python: a route whose handler names a verb keeps its handler; a class only spreads into its methods when the verb is open
    let d = analyze(&[("main.py", "from flask import Flask\n\napp = Flask(__name__)\n\nclass Ctl:\n    def get(self):\n        return 1\n\n@app.get(\"/c\")\ndef c():\n    return Ctl()\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(route_lines(&d), ["GET /c c()"]);
    // Starlette: an endpoint class in a Route answers with its verb methods, each under its own verb; a method that names no verb stays under ANY
    let d = analyze(&[("main.py", "from starlette.applications import Starlette\nfrom starlette.endpoints import HTTPEndpoint\nfrom starlette.routing import Route\n\nclass UserEndpoint(HTTPEndpoint):\n    async def get(self, request):\n        return 1\n\n    async def post(self, request):\n        return 2\n\n    async def list(self, request):\n        return 3\n\napp = Starlette(routes=[Route(\"/users\", UserEndpoint)])\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(route_lines(&d), ["ANY /users UserEndpoint.list()", "GET /users UserEndpoint.get()", "POST /users UserEndpoint.post()"]);
    // a list of pairs handed to a call that is not an application is not a route table
    let d = analyze(&[("main.py", "import tornado.web\n\ndef run(x):\n    return dict([(\"/a\", x)])\n\nif __name__ == \"__main__\":\n    run(1)\n")]);
    assert_eq!(procs(&d)[0]["routes"].as_array().map_or(0, |r| r.len()), 0);
}

/// the database labels of a route that runs `body`, with `params`, `imports` in main.py and `models` in models.py
fn py_route_db(models: &str, imports: &str, params: &str, body: &str) -> Vec<String> {
    let main = format!("from fastapi import FastAPI\n{imports}\n\napp = FastAPI()\n\n@app.get(\"/x\")\nasync def x({params}):\n{body}\n");
    let mut files = vec![("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"), ("main.py", main.as_str())];
    if !models.is_empty() {
        files.push(("models.py", models));
    }
    db_steps(&analyze(&files), "/x")
}

#[test]
fn python_mongoengine_orm() {
    // MongoEngine: a Document model, queried through objects(..), an instance saved and deleted
    let got = py_route_db("from mongoengine import Document, StringField\n\nclass User(Document):\n    name = StringField()\n", "from models import User", "",
                    "    u = User(name=\"x\")\n    u.save()\n    u.delete()\n    return User.objects(name=\"x\").first()");
    assert_eq!(got, ["writes User", "reads User"]);
}

#[test]
fn python_beanie_orm() {
    // Beanie: a Document from beanie, found and fetched by id, an instance inserted and saved
    let got = py_route_db("from beanie import Document\n\nclass User(Document):\n    name: str\n", "from models import User", "",
                    "    u = User(name=\"x\")\n    await u.insert()\n    await u.save()\n    await User.get(1)\n    return await User.find(User.name == \"x\").to_list()");
    assert_eq!(got, ["writes User", "reads User"]);
}

#[test]
fn python_odmantic_orm() {
    // ODMantic: the engine reads and writes the model it is handed
    let got = py_route_db("from odmantic import Model\n\nclass User(Model):\n    name: str\n", "from odmantic import AIOEngine\nfrom models import User", "",
                    "    engine = AIOEngine()\n    await engine.find(User, User.name == \"x\")\n    await engine.find_one(User)\n    await engine.save(User(name=\"y\"))\n    await engine.delete(u)");
    assert_eq!(got, ["reads User", "writes User", "writes database"]);
}

#[test]
fn python_piccolo_orm() {
    // Piccolo: a Table, read with select/objects, written with insert/update/delete and an instance's save
    let got = py_route_db("from piccolo.table import Table\nfrom piccolo.columns import Varchar\n\nclass User(Table):\n    name = Varchar()\n", "from models import User", "",
                    "    await User.select().where(User.name == \"x\")\n    await User.objects().first()\n    await User.insert(User(name=\"x\"))\n    await User.update({User.name: \"y\"}).where(User.id == 1)\n    await User.delete().where(User.id == 1)\n    u = User(name=\"z\")\n    await u.save()");
    assert_eq!(got, ["reads User", "writes User"]);
}

#[test]
fn python_ormar_orm() {
    // ormar: queries through objects, created through objects, an instance updated
    let got = py_route_db("import ormar\n\nclass User(ormar.Model):\n    name: str = ormar.String(max_length=100)\n", "from models import User", "",
                    "    await User.objects.get(id=1)\n    await User.objects.filter(name=\"x\").all()\n    await User.objects.create(name=\"y\")\n    u = User(name=\"z\")\n    await u.update(name=\"w\")\n    await u.delete()");
    assert_eq!(got, ["reads User", "writes User"]);
}

#[test]
fn python_pymongo_orm() {
    // PyMongo: a collection handed in
    let got = py_route_db("", "from pymongo.collection import Collection", "users: Collection",
                    "    users.find_one({\"_id\": 1})\n    users.count_documents({})\n    users.aggregate([])\n    users.insert_one({\"a\": 1})\n    users.update_many({}, {})\n    users.delete_one({})\n    users.replace_one({}, {})");
    assert_eq!(got, ["reads database", "writes database"]);
    let got = py_route_db("", "from pymongo.database import Database", "db: Database", "    return db.users.find({})");
    assert_eq!(got, ["reads database"]);
}

#[test]
fn python_motor_orm() {
    // Motor: the same calls on an async collection
    let got = py_route_db("", "from motor.motor_asyncio import AsyncIOMotorCollection", "users: AsyncIOMotorCollection",
                    "    await users.insert_many([])\n    await users.delete_many({})\n    await users.update_one({}, {})");
    assert_eq!(got, ["writes database"]);
}

#[test]
fn python_asyncpg_orm() {
    // asyncpg: fetches read, execute writes
    let got = py_route_db("", "import asyncpg", "conn: asyncpg.Connection", "    await conn.fetch(\"SELECT 1\")\n    await conn.fetchrow(\"SELECT 1\")\n    await conn.fetchval(\"SELECT 1\")");
    assert_eq!(got, ["reads database"]);
    let got = py_route_db("", "import asyncpg", "pool: asyncpg.Pool", "    await pool.execute(\"DELETE FROM t\")\n    await pool.executemany(\"INSERT INTO t VALUES ($1)\", rows)");
    assert_eq!(got, ["writes database"]);
}

#[test]
fn python_psycopg_orm() {
    // psycopg: the SQL text is not read, so execute counts as a write; fetching rows reads
    let got = py_route_db("", "import psycopg", "cur: psycopg.Cursor", "    cur.execute(\"SELECT 1\")\n    return cur.fetchall()");
    assert_eq!(got, ["writes database", "reads database"]);
    let got = py_route_db("", "import psycopg", "conn: psycopg.Connection", "    return conn.execute(\"SELECT 1\").fetchone()");
    assert_eq!(got, ["writes database"]);
}

#[test]
fn python_db_libraries_leave_plain_values_alone() {
    // values that are not a database add no step, even in a file written for these libraries
    for imports in ["import pymongo\nfrom pymongo.collection import Collection", "import asyncpg", "import psycopg", "from motor.motor_asyncio import AsyncIOMotorCollection",
                    "from mongoengine import Document", "from beanie import Document", "from odmantic import AIOEngine", "from piccolo.table import Table", "import ormar"] {
        let got = py_route_db("", imports, "d: dict, cache, items: list", "    d.get(\"k\")\n    cache.delete(key)\n    items.find(\"x\")\n    \"abc\".find(\"x\")");
        assert_eq!(got, Vec::<String>::new(), "{imports}");
    }
}

#[test]
fn python_libraries_are_catalogued() {
    // each library is listed under what it is
    for (lib, label) in [("mongoengine", "Database"), ("beanie", "Database"), ("odmantic", "Database"), ("piccolo", "Database"), ("ormar", "Database"),
                         ("pymongo", "Database"), ("motor", "Database"), ("asyncpg", "Database"), ("psycopg", "Database"),
                         ("falcon", "Web framework"), ("pyramid", "Web framework"), ("tornado", "Web framework"), ("bottle", "Web framework"),
                         ("quart", "Web framework"), ("ninja", "Web framework")] {
        let d = analyze(&[("main.py", format!("import {lib}\n\nif __name__ == \"__main__\":\n    {lib}.run()\n").as_str())]);
        let labels: Vec<&str> = d["repo"]["stack"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap()).collect();
        assert_eq!(labels, [label], "{lib}");
    }
}

#[test]
fn a_name_known_only_as_an_import_does_not_count_from_a_dependency_file() {
    let stack = |files: &[(&str, &str)]| -> Vec<String> {
        let d = analyze(files);
        d["repo"]["stack"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap().to_string()).collect()
    };
    // the ninja build tool listed for a C++ extension is not Django Ninja
    let got = stack(&[("requirements.txt", "ninja\nnumpy\n"), ("main.py", "def run():\n    return 1\n\nif __name__ == \"__main__\":\n    run()\n")]);
    assert!(!got.iter().any(|l| l == "Web framework"), "{got:?}");
    let got = stack(&[("pyproject.toml", "[project]\nname = \"x\"\ndependencies = [\"ninja>=1.11\"]\n"), ("main.py", "def run():\n    return 1\n\nif __name__ == \"__main__\":\n    run()\n")]);
    assert!(!got.iter().any(|l| l == "Web framework"), "{got:?}");
    // imported, with its package listed: a web framework
    let got = stack(&[
        ("requirements.txt", "django-ninja\n"),
        ("main.py", "from ninja import NinjaAPI\n\napi = NinjaAPI()\n\n@api.get(\"/users\")\ndef users(request):\n    return []\n\nif __name__ == \"__main__\":\n    run(api)\n"),
    ]);
    assert_eq!(got, ["Web framework"]);
    // the package listed alone still counts by its package name
    let got = stack(&[("requirements.txt", "django-ninja\n"), ("main.py", "def run():\n    return 1\n\nif __name__ == \"__main__\":\n    run()\n")]);
    assert_eq!(got, ["Web framework"]);
}

#[test]
fn go_frameworks() {
    let go = |imports: &str, body: &str| {
        let src = format!("package main\n\nimport (\n\t\"net/http\"\n{imports})\n\nfunc main() {{\n{body}}}\n\nfunc listUsers() {{}}\n");
        let d = analyze(&[("main.go", src.as_str())]);
        (route_paths(&procs(&d)[0]), route_first(&d, &route_paths(&procs(&d)[0]).first().map(|r| r.split(' ').nth(1).unwrap().to_string()).unwrap_or_default()))
    };
    // Gin: a group, then the route
    let (r, first) = go("\t\"github.com/gin-gonic/gin\"\n", "\tr := gin.Default()\n\tv1 := r.Group(\"/v1\")\n\tv1.GET(\"/users\", listUsers)\n\tr.Run()\n");
    assert_eq!((r, first.as_deref()), (vec!["GET /v1/users".to_string()], Some("listUsers()")));
    // Echo
    let (r, _) = go("\t\"github.com/labstack/echo/v4\"\n", "\te := echo.New()\n\te.GET(\"/users\", listUsers)\n\te.Start(\":80\")\n");
    assert_eq!(r, ["GET /users"]);
    // Fiber
    let (r, _) = go("\t\"github.com/gofiber/fiber/v2\"\n", "\tapp := fiber.New()\n\tapp.Get(\"/users\", listUsers)\n\tapp.Listen(\":80\")\n");
    assert_eq!(r, ["GET /users"]);
    // gorilla/mux: the verb comes after the call
    let (r, first) = go("\t\"github.com/gorilla/mux\"\n", "\tr := mux.NewRouter()\n\tr.HandleFunc(\"/users\", listUsers).Methods(\"GET\")\n\thttp.ListenAndServe(\":80\", r)\n");
    assert_eq!((r, first.as_deref()), (vec!["GET /users".to_string()], Some("listUsers()")));
}

#[test]
fn go_methods_of_unexported_types_answer_the_routes_they_are_handed_to() {
    // `handler *placementHandler` is a parameter, and `handler.getPlacements` is handed over, not called
    let d = analyze(&[
        ("go.mod", "module shop\n"),
        ("main.go", "package main\n\nimport (\n\t\"github.com/labstack/echo/v4\"\n\t\"shop/api\"\n)\n\nfunc main() {\n\te := echo.New()\n\tapi.NewPlacementHandler(e)\n\te.Start(\":80\")\n}\n"),
        // two functions call their parameter `handler`, each of a different type
        ("api/routes.go", "package api\n\nimport \"github.com/labstack/echo/v4\"\n\nfunc placementRoutes(e *echo.Echo, handler *placementHandler) {\n\tg := e.Group(\"/v1\")\n\tg.GET(\"/placements\", handler.getPlacements)\n}\n\nfunc tagRoutes(e *echo.Echo, handler *tagHandler) {\n\te.GET(\"/v1/tags\", handler.listTags)\n}\n"),
        ("api/placement_handler.go", "package api\n\nimport \"github.com/labstack/echo/v4\"\n\ntype placementHandler struct{}\n\nfunc NewPlacementHandler(e *echo.Echo) {\n\thandler := &placementHandler{}\n\tplacementRoutes(e, handler)\n}\n\nfunc (ph *placementHandler) getPlacements(c echo.Context) error {\n\treturn nil\n}\n\ntype tagHandler struct{}\n\nfunc (th *tagHandler) listTags(c echo.Context) error {\n\treturn nil\n}\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /v1/placements", "GET /v1/tags"]);
    assert_eq!(route_first(&d, "/v1/placements").as_deref(), Some("placementHandler.getPlacements()"));
    assert_eq!(route_first(&d, "/v1/tags").as_deref(), Some("tagHandler.listTags()"));
}

#[test]
fn go_orms() {
    let steps = |imports: &str, body: &str| {
        let src = format!("package main\n\nimport (\n{imports})\n\nfunc main() {{ run(nil) }}\n\nfunc run(db {body}\n");
        db_steps(&analyze(&[("main.go", src.as_str())]), "")
    };
    // GORM: a query chain ending in a read, then a create
    // (the created value is a struct that embeds gorm.Model: a model the call names)
    let got = steps("\t\"gorm.io/gorm\"\n", "*gorm.DB) {\n\tvar users []User\n\tdb.Where(\"name = ?\", \"x\").Find(&users)\n\tdb.Create(&User{})\n}\n\ntype User struct {\n\tgorm.Model\n}\n");
    assert_eq!(got, ["reads database", "writes User"]);
    // sqlx
    let got = steps("\t\"github.com/jmoiron/sqlx\"\n", "*sqlx.DB) {\n\tvar n int\n\tdb.Get(&n, \"SELECT count(*) FROM t\")\n\tdb.MustExec(\"DELETE FROM t\")\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // database/sql
    let got = steps("\t\"database/sql\"\n", "*sql.DB) {\n\tdb.QueryRow(\"SELECT 1\")\n\tdb.Exec(\"DELETE FROM t\")\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
}

#[test]
fn more_go_frameworks() {
    // every route of a one-file program: "VERB /path handler"
    let go = |imports: &str, body: &str| {
        let src = format!("package main\n\nimport (\n{imports})\n\nfunc main() {{\n{body}}}\n\nfunc listUsers() {{}}\n\nfunc getUser() {{}}\n");
        let d = analyze(&[("main.go", src.as_str())]);
        route_paths(&procs(&d)[0]).into_iter().map(|r| {
            let first = route_first(&d, r.split(' ').nth(1).unwrap()).unwrap_or_default();
            format!("{r} {first}")
        }).collect::<Vec<_>>()
    };
    // Beego: a route function on the web package, and a namespace-free controller-less handler
    let r = go("\t\"github.com/beego/beego/v2/server/web\"\n", "\tweb.Get(\"/users\", listUsers)\n\tweb.Post(\"/users/:id\", getUser)\n\tweb.Run()\n");
    assert_eq!(r, ["GET /users listUsers()", "POST /users/:id getUser()"]);
    // Beego: a controller registered on a path answers with its verb methods
    let src = "package main\n\nimport \"github.com/beego/beego/v2/server/web\"\n\ntype UserController struct {\n\tweb.Controller\n}\n\nfunc (c *UserController) Get() {}\n\nfunc (c *UserController) Post() {}\n\nfunc main() {\n\tweb.Router(\"/users\", &UserController{})\n\tweb.Run()\n}\n";
    let d = analyze(&[("main.go", src)]);
    assert_eq!(route_paths(&procs(&d)[0]), ["ANY /users"]);
    let names: Vec<String> = procs(&d)[0]["routes"][0]["steps"].as_array().unwrap().iter().map(|s| s["n"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["UserController.Get()", "UserController.Post()"]);
    // Iris: a party is a group with a prefix
    let r = go("\t\"github.com/kataras/iris/v12\"\n", "\tapp := iris.New()\n\tusers := app.Party(\"/users\")\n\tusers.Get(\"/{id}\", getUser)\n\tapp.Listen(\":8080\")\n");
    assert_eq!(r, ["GET /users/{id} getUser()"]);
    // Hertz: Gin-style verbs on a group
    let r = go("\t\"github.com/cloudwego/hertz/pkg/app/server\"\n", "\th := server.Default()\n\tv1 := h.Group(\"/v1\")\n\tv1.GET(\"/users\", listUsers)\n\th.Spin()\n");
    assert_eq!(r, ["GET /v1/users listUsers()"]);
}

#[test]
fn more_go_orms() {
    let steps = |imports: &str, body: &str| {
        let src = format!("package main\n\nimport (\n{imports})\n\nfunc main() {{ run(nil) }}\n\nfunc run(db {body}\n");
        db_steps(&analyze(&[("main.go", src.as_str())]), "")
    };
    // pgx: queries read, Exec and CopyFrom write
    let got = steps("\t\"github.com/jackc/pgx/v5/pgxpool\"\n", "*pgxpool.Pool) {\n\tdb.QueryRow(ctx, \"SELECT 1\")\n\tdb.Exec(ctx, \"DELETE FROM t\")\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    let got = steps("\t\"github.com/jackc/pgx/v5\"\n", "*pgx.Conn) {\n\tdb.CopyFrom(ctx, pgx.Identifier{\"t\"}, cols, rows)\n}\n");
    assert_eq!(got, ["writes database"]);
    // xorm: a struct with xorm tags is a model the call names
    let got = steps("\t\"xorm.io/xorm\"\n", "*xorm.Engine) {\n\tvar n int\n\tdb.Where(\"id = ?\", 1).Get(&n)\n\tdb.Insert(&User{})\n}\n\ntype User struct {\n\tID int64 `xorm:\"pk autoincr\"`\n}\n");
    assert_eq!(got, ["reads database", "writes User"]);
    // Beego ORM: the session comes from orm.NewOrm()
    let got = steps("\t\"github.com/beego/beego/v2/client/orm\"\n", "int) {\n\to := orm.NewOrm()\n\to.QueryTable(\"user\").Filter(\"id\", 1).One(&u)\n\to.Delete(&u)\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // values that are not a session add no step, even in a file written for an ORM
    let got = steps("\t\"github.com/jackc/pgx/v5/pgxpool\"\n", "*pgxpool.Pool) {\n\tm.Get(\"k\")\n\tcache.Delete(k)\n\tm.Query(\"x\")\n}\n");
    assert_eq!(got, Vec::<String>::new());
    // each library is listed under what it is
    for (lib, label) in [("xorm.io/xorm", "Database"), ("github.com/beego/beego/v2/client/orm", "Database"), ("github.com/sqlc-dev/sqlc", "Database"),
                         ("github.com/kataras/iris/v12", "Web framework"), ("github.com/cloudwego/hertz/pkg/app/server", "Web framework"),
                         ("github.com/beego/beego/v2/server/web", "Web framework")] {
        let d = analyze(&[("main.go", format!("package main\n\nimport \"{lib}\"\n\nfunc main() {{}}\n").as_str())]);
        let labels: Vec<&str> = d["repo"]["stack"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap()).collect();
        assert_eq!(labels, [label], "{lib}");
    }
}

#[test]
fn sqlc_queries() {
    let generated = ("db/db.go", "package db\n\ntype DBTX interface{}\n\nfunc New(db DBTX) *Queries {\n\treturn &Queries{db: db}\n}\n\ntype Queries struct {\n\tdb DBTX\n}\n");
    // the generated method names say what they do: Get/List read, Create/Delete write
    let d = analyze(&[
        generated,
        ("main.go", "package main\n\nimport \"app/db\"\n\nfunc main() { run(nil) }\n\nfunc run(q *db.Queries) {\n\tq.GetUser(ctx, 1)\n\tq.ListUsers(ctx)\n\tq.CreateUser(ctx, \"x\")\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["reads database", "writes database"]);
    // the handle made by the generated constructor, held in a struct field
    let d = analyze(&[
        generated,
        ("main.go", "package main\n\nimport \"app/db\"\n\ntype Server struct {\n\tq *db.Queries\n}\n\nfunc main() {\n\ts := &Server{q: db.New(nil)}\n\ts.remove()\n}\n\nfunc (s *Server) remove() {\n\ts.q.DeleteUser(ctx, 1)\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["writes database"]);
    // a prefix only counts on a session: a map's Get and a cache's Delete are not the database,
    // and another package's New() does not hand out the generated Queries
    let d = analyze(&[
        generated,
        ("main.go", "package main\n\nimport (\n\t\"app/db\"\n\t\"github.com/gofiber/fiber/v2\"\n)\n\nfunc main() { run(nil) }\n\nfunc run(q *db.Queries) {\n\tapp := fiber.New()\n\tapp.Get(\"k\")\n\tm.Get(\"k\")\n\tcache.Delete(k)\n\tq.Getaway()\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
}

#[test]
fn a_session_getter_counts_only_where_the_call_names_it() {
    let generated = ("db/db.go", "package db\n\ntype DBTX interface{}\n\nfunc New(db DBTX) *Queries {\n\treturn &Queries{db: db}\n}\n\ntype Queries struct {\n\tdb DBTX\n}\n");
    // the generated constructor, called through its package from elsewhere and bare inside its own package
    let d = analyze(&[
        generated,
        ("go.mod", "module app\n"),
        ("db/seed.go", "package db\n\nfunc Seed(conn DBTX) {\n\tqueries := New(conn)\n\tqueries.CreateUser(ctx, \"x\")\n}\n"),
        ("main.go", "package main\n\nimport \"app/db\"\n\nfunc main() { run(nil) }\n\nfunc run(conn db.DBTX) {\n\tq := db.New(conn)\n\tq.GetUser(ctx, 1)\n\tdb.Seed(conn)\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["reads database", "writes database"]);
    // another package's New: called through that package, or bare inside it, hands out no session
    let d = analyze(&[
        generated,
        ("other/other.go", "package other\n\ntype Thing struct{}\n\nfunc New() *Thing { return &Thing{} }\n\nfunc Use() {\n\tt := New()\n\tt.GetFoo()\n}\n"),
        ("main.go", "package main\n\nimport (\n\t\"errors\"\n\t\"app/other\"\n)\n\nfunc main() { run() }\n\nfunc run() {\n\tx := errors.New(\"e\")\n\tx.GetFoo()\n\tt := other.New()\n\tt.GetFoo()\n\tother.Use()\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
    // the getters other languages already had keep working: Python bare import, Go through its package (a_session_returned_by_a_function_is_a_session)
    let d = analyze(&[
        ("deps.py", "from sqlalchemy.orm import Session\n\ndef get_db() -> Session:\n    return Session()\n"),
        ("main.py", "from sqlalchemy.orm import Session\nfrom deps import get_db\n\ndef run():\n    db = get_db()\n    db.commit()\n\nif __name__ == '__main__':\n    run()\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["writes database"]);
}

#[test]
fn a_package_qualified_handle_changes_nothing_outside_go() {
    // Python: `with orm.Session() as s` and `import x.Session` name a type through dots but declare no session variable
    let d = analyze(&[
        ("main.py", "from sqlalchemy import orm\nimport sqlalchemy.orm.Session\n\ndef run(d):\n    with orm.Session() as s:\n        d.get(\"a\")\n        s.commit()\n\nif __name__ == '__main__':\n    run({})\n"),
    ]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
    // Rust paths use `::`, PHP variables `$`: same output as before the change
    let rust = analyze(&[("src/main.rs", "use sqlx::PgPool;\n\nfn main() { read(todo!()); }\n\nasync fn read(pool: &sqlx::PgPool) {\n    let other = cfg.PgPool;\n    sqlx::query(\"SELECT 1\").fetch_all(pool).await;\n    other.fetch_all();\n}\n")]);
    assert_eq!(db_steps(&rust, ""), ["reads database"]);
    let php = analyze(&[("public/index.php", "<?php\nfunction q(\\PDO $pdo, $cfg) { $x = $cfg . PDO; $x->query('a'); return $pdo->query('SELECT 1'); }\nq(null, 1);\n")]);
    assert_eq!(db_steps(&php, ""), ["reads database"]);
}

#[test]
fn go_controller_data_does_not_reach_other_languages() {
    // JS: express.Router() takes no path, so it is a router, not a route
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"main.js\",\"dependencies\":{\"express\":\"4\"}}"),
        ("main.js", "const express = require('express')\nconst app = express()\nconst r = express.Router()\nr.get('/a', (req, res) => res.send(1))\napp.use(r)\napp.listen(3000)\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /a"]);
    // Python: a class view's capitalised helper is not a verb method
    let d = analyze(&[
        ("main.py", "import urls\nimport views\n\nif __name__ == \"__main__\":\n    urls.urlpatterns\n"),
        ("urls.py", "from django.urls import path\nfrom views import ItemView\n\nurlpatterns = [path(\"items/\", ItemView.as_view())]\n"),
        ("views.py", "class ItemView:\n    def post(self, request):\n        return 1\n\n    def Get(self):\n        return 2\n"),
    ]);
    let names: Vec<String> = procs(&d)[0]["routes"][0]["steps"].as_array().unwrap().iter().map(|s| s["n"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["ItemView.post()"]);
    // PHP: a controller class with Get/Post methods registered as a whole: only the usual resource actions
    let (d, _) = one_route_set(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../routes/web.php';\n"),
        ("routes/web.php", "<?php\nRoute::get('/items', ItemController::class);\n"),
        ("app/Http/Controllers/ItemController.php", "<?php\nclass ItemController {\n    public function Get() {}\n    public function Post() {}\n}\n"),
    ]);
    assert_eq!(route_first(&d, "/items"), None);
}

#[test]
fn rust_frameworks() {
    // Rocket: an attribute with a typed segment
    let (_, r) = one_route_set(&[("src/main.rs", "#[get(\"/users/<id>\")]\nfn user(id: u32) {}\n\nfn main() {}\n")]);
    assert_eq!(r, ["GET /users/<id>"]);
    // Poem: .at(path, get(handler))
    let (d, r) = one_route_set(&[("src/main.rs", "use poem::{get, Route};\n\nfn main() {\n    let app = Route::new().at(\"/users\", get(list_users));\n}\n\nfn list_users() {}\n")]);
    assert_eq!(r, ["GET /users"]);
    assert_eq!(route_first(&d, "/users").as_deref(), Some("list_users()"));
}

#[test]
fn rust_orms() {
    let steps = |src: &str| db_steps(&analyze(&[("src/main.rs", src)]), "");
    // SQLx: the pool is an argument of the call that runs the query
    let got = steps("use sqlx::PgPool;\n\nfn main() { read(todo!()); write(todo!()); }\n\nasync fn read(pool: &PgPool) {\n    sqlx::query(\"SELECT 1\").fetch_all(pool).await;\n}\n\nasync fn write(pool: &PgPool) {\n    sqlx::query(\"DELETE FROM t\").execute(pool).await;\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // Diesel: a table, then load with the connection
    let got = steps("use diesel::prelude::*;\n\nfn main() { read(todo!()); write(todo!()); }\n\nfn read(conn: &mut PgConnection) {\n    users.filter(id.eq(1)).load::<User>(conn);\n}\n\nfn write(conn: &mut PgConnection) {\n    diesel::insert_into(users).values(&u).execute(conn);\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // SeaORM
    let got = steps("use sea_orm::DatabaseConnection;\n\nfn main() { read(todo!()); }\n\nasync fn read(db: &DatabaseConnection) {\n    User::find().all(db).await;\n}\n");
    assert_eq!(got, ["reads database"]);
}

#[test]
fn more_rust_frameworks() {
    // every route of a one-file program: "VERB /path handler"
    let rs = |body: &str| {
        let src = format!("{body}\n\nfn list() {{}}\n\nfn show() {{}}\n\nfn create() {{}}\n");
        let d = analyze(&[("src/main.rs", src.as_str())]);
        let routes = procs(&d)[0]["routes"].as_array().cloned().unwrap_or_default();
        sorted(routes.iter().map(|r| {
            let first = r["steps"].as_array().and_then(|s| s.first()).map(|s| s["n"].as_str().unwrap().to_string()).unwrap_or_default();
            format!("{} {} {first}", r["verb"].as_str().unwrap(), r["path"].as_str().unwrap())
        }).collect())
    };
    // Tide: verbs chained after the path, each with its handler; the path alone is no route
    let r = rs("use tide::Request;\n\nfn main() {\n    let mut app = tide::new();\n    app.at(\"/users/:id\").get(show).post(create);\n    app.listen(\"127.0.0.1:8080\");\n}");
    assert_eq!(r, ["GET /users/:id show()", "POST /users/:id create()"]);
    // Salvo: a router with a path, its verbs chained, nested under another router's path by push
    let r = rs("use salvo::prelude::*;\n\nfn main() {\n    let router = Router::with_path(\"api\")\n        .push(Router::with_path(\"users\").get(list).post(create))\n        .push(Router::with_path(\"users/<id>\").get(show));\n    serve(router);\n}");
    assert_eq!(r, ["GET /api/users list()", "GET /api/users/<id> show()", "POST /api/users create()"]);
    // Loco: a prefix, then routes added under it with the verb wrapping the handler
    let r = rs("use loco_rs::prelude::*;\n\nfn main() {\n    routes();\n}\n\npub fn routes() -> Routes {\n    Routes::new()\n        .prefix(\"users\")\n        .add(\"/\", get(list))\n        .add(\"/:id\", get(show))\n        .add(\"/\", post(create))\n}");
    assert_eq!(r, ["GET /users list()", "GET /users/:id show()", "POST /users create()"]);
    // Warp: a filter chain names its path in a macro and its verb in a filter; it is not read, so no route is claimed
    let r = rs("use warp::Filter;\n\nfn main() {\n    let users = warp::path!(\"users\" / u32).and(warp::get()).and_then(show);\n    warp::serve(users);\n}");
    assert_eq!(r, Vec::<String>::new());
    // a lookup or a builder that is not a router makes no route, even after a path-like call
    let r = rs("use tide::Request;\n\nfn main() {\n    let m = std::collections::HashMap::new();\n    m.get(list);\n    url.join(\"/a\").get(show);\n    cfg.path(\"a\").clone().get(show);\n}");
    assert_eq!(r, Vec::<String>::new());
    // each library is listed under what it is
    for (lib, label) in [("salvo", "Web framework"), ("tide", "Web framework"), ("loco_rs", "Web framework")] {
        let d = analyze(&[("src/main.rs", format!("use {lib}::prelude::*;\n\nfn main() {{}}\n").as_str())]);
        let labels: Vec<&str> = d["repo"]["stack"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap()).collect();
        assert_eq!(labels, [label], "{lib}");
    }
}

#[test]
fn nested_tide_paths_join() {
    let d = analyze(&[("src/main.rs", "use tide::Request;\n\nfn main() {\n    let mut app = tide::new();\n    app.at(\"/users\").at(\"/:id\").get(show);\n    app.at(\"/api\").at(\"/v1\").at(\"/items\").get(list).post(create);\n    app.listen(\"127.0.0.1:8080\");\n}\n\nfn list() {}\n\nfn show() {}\n\nfn create() {}\n")]);
    assert_eq!(route_paths(&procs(&d)[0]), ["GET /api/v1/items", "GET /users/:id", "POST /api/v1/items"]);
    assert_eq!(route_first(&d, "/users/:id").as_deref(), Some("show()"));
}

#[test]
fn a_crate_qualified_handle_type_belongs_to_its_crate() {
    let steps = |src: &str| db_steps(&analyze(&[("src/main.rs", src)]), "");
    // another crate's Client or Connection is no database session, even in a file written for one
    let got = steps("use tokio_postgres::Client;\n\nfn main() { run(todo!()); }\n\nasync fn run(http: &reqwest::Client) {\n    http.get(url).query(&[(\"a\", 1)]).send().await;\n    http.execute(req).await;\n}\n");
    assert_eq!(got, Vec::<String>::new());
    let got = steps("use rusqlite::params;\n\nfn main() { run(todo!()); }\n\nfn run(conn: &redis::Connection) {\n    conn.execute(\"x\");\n    conn.query_row(\"y\");\n}\n");
    assert_eq!(got, Vec::<String>::new());
    // its own crate's, named through the path or imported bare, still is
    let got = steps("use tokio_postgres::NoTls;\n\nfn main() { run(todo!()); }\n\nasync fn run(client: &tokio_postgres::Client) {\n    client.query_one(\"SELECT 1\", &[]).await;\n}\n");
    assert_eq!(got, ["reads database"]);
    let got = steps("use tokio_postgres::Client;\n\nfn main() { run(todo!()); }\n\nasync fn run(client: &Client) {\n    client.execute(\"DELETE FROM t\", &[]).await;\n}\n");
    assert_eq!(got, ["writes database"]);
}

#[test]
fn a_diesel_pool_wrapped_by_another_crate_is_a_session() {
    let steps = |src: &str| db_steps(&analyze(&[("src/main.rs", src)]), "");
    // r2d2 and bb8 wrap Diesel's connection: the pool handed to the query is still the session
    for pool in ["r2d2::Pool<ConnectionManager<PgConnection>>", "bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>"] {
        let got = steps(&format!("use diesel::prelude::*;\n\nfn main() {{ run(todo!()); }}\n\nfn run(pool: &{pool}) {{\n    users.load::<User>(&mut pool.get().unwrap());\n}}\n"));
        assert_eq!(got, ["reads database"], "{pool}");
    }
    // the same wrapper in a file not written for Diesel is nothing
    for lib in ["sqlx::Row", "rusqlite::params"] {
        let got = steps(&format!("use {lib};\n\nfn main() {{ run(todo!()); }}\n\nfn run(pool: &r2d2::Pool<M>) {{\n    sqlx::query(\"SELECT 1\").fetch_all(pool);\n    pool.execute(\"x\");\n    pool.query_row(\"y\");\n}}\n"));
        assert_eq!(got, Vec::<String>::new(), "{lib}");
    }
    // a wrapper crate is not a crate path for every type: another crate's Client is still not Diesel's
    let got = steps("use diesel::prelude::*;\nuse tokio_postgres::Client;\n\nfn main() { run(todo!()); }\n\nasync fn run(http: &reqwest::Client) {\n    http.execute(req).await;\n}\n");
    assert_eq!(got, Vec::<String>::new());
}

#[test]
fn rust_chain_routes_stay_in_rust() {
    // the same chains in JavaScript and Python are not routes: the chain reading is Rust's
    let d = analyze(&[("server.js", "const express = require('express')\nconst app = express()\napp.at('/users').get(show)\nRoutes.prefix('api').add('/x', get(show))\napp.listen(3000)\nfunction show() {}\n")]);
    assert_eq!(route_paths(&procs(&d)[0]), ["ANY /users"]);
    let d = analyze(&[("main.py", "from flask import Flask\n\napp = Flask(__name__)\nrouter.prefix(\"api\").add(\"/x\", get(show))\n\ndef show():\n    return 1\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(route_paths(&procs(&d)[0]), Vec::<String>::new());
}

#[test]
fn more_rust_orms() {
    let steps = |src: &str| db_steps(&analyze(&[("src/main.rs", src)]), "");
    // rusqlite: query_row and prepare read, execute and execute_batch write
    // (`stmt.query_map(..)` alone is not read: the statement is not a declared handle, the `prepare` on the connection is)
    let got = steps("use rusqlite::{params, Connection};\n\nfn main() { read(todo!()); write(todo!()); }\n\nfn read(conn: &Connection) {\n    conn.query_row(\"SELECT 1\", [], |r| r.get(0));\n    let mut stmt = conn.prepare(\"SELECT id FROM t\").unwrap();\n}\n\nfn write(conn: &Connection) {\n    conn.execute(\"DELETE FROM t\", params![]);\n    conn.execute_batch(\"VACUUM\");\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // tokio-postgres: a client and a transaction
    let got = steps("use tokio_postgres::{Client, Transaction};\n\nfn main() { read(todo!()); write(todo!()); }\n\nasync fn read(client: &Client) {\n    client.query_one(\"SELECT 1\", &[]).await;\n    client.query_opt(\"SELECT 1\", &[]).await;\n}\n\nasync fn write(tx: Transaction<'_>) {\n    tx.batch_execute(\"DELETE FROM t\").await;\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // Rbatis: raw SQL on the handle, and the model functions crud! defines, which take the handle
    let got = steps("use rbatis::RBatis;\n\nfn main() { read(todo!()); write(todo!()); }\n\nasync fn read(rb: &RBatis) {\n    let n: i64 = rb.query_decode(\"SELECT count(1) FROM t\", vec![]).await.unwrap();\n    Activity::select_all(rb).await;\n}\n\nasync fn write(rb: &RBatis) {\n    Activity::insert(rb, &a).await;\n    Activity::delete_by_map(rb, value!{\"id\": 1}).await;\n}\n\ncrud!(Activity {});\n");
    assert_eq!(got, ["reads database", "writes database"]);
    let got = steps("use rbatis::RBatis;\n\nfn main() { write(todo!()); }\n\nasync fn write(rb: &RBatis) {\n    rb.exec(\"DELETE FROM t\", vec![]).await;\n}\n");
    assert_eq!(got, ["writes database"]);
    // mongodb: a typed collection
    let got = steps("use mongodb::Collection;\n\nfn main() { read(todo!()); write(todo!()); }\n\nasync fn read(users: Collection<User>) {\n    users.find_one(doc! { \"id\": 1 }).await;\n    users.count_documents(doc! {}).await;\n}\n\nasync fn write(users: Collection<User>) {\n    users.insert_one(u).await;\n    users.find_one_and_update(f, u).await;\n}\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // values that are not a session add no step, even in a file written for these crates
    for lib in ["rusqlite::Connection", "tokio_postgres::Client", "rbatis::RBatis", "mongodb::Collection"] {
        let got = steps(&format!("use {lib};\nuse std::collections::{{HashMap, HashSet}};\n\nfn main() {{ run(); }}\n\nfn run() {{\n    let map: HashMap<u32, u32> = HashMap::new();\n    map.get(&k);\n    v.iter();\n    cache.remove(&k);\n    set.insert(x);\n    map.insert(1, 2);\n    items.update(x);\n    job.execute(x);\n}}\n"));
        assert_eq!(got, Vec::<String>::new(), "{lib}");
    }
    // a real session in the same function: generic names called on other receivers still add no step
    for (lib, session) in [("rusqlite::Connection", "conn: &Connection"), ("tokio_postgres::Client", "client: &Client"),
                           ("rbatis::RBatis", "rb: &RBatis"), ("mongodb::Collection", "users: Collection<User>")] {
        let got = steps(&format!("use {lib};\nuse std::collections::HashMap;\n\nfn main() {{ run(todo!()); }}\n\nfn run({session}) {{\n    let mut map: HashMap<u32, u32> = HashMap::new();\n    map.insert(k, v);\n    cache.remove(&k);\n    set.insert(x);\n    items.update(x);\n    job.execute(x);\n}}\n"));
        assert_eq!(got, Vec::<String>::new(), "{lib}");
    }
    // each library is listed under what it is
    for lib in ["rusqlite", "tokio_postgres", "rbatis", "mongodb"] {
        let d = analyze(&[("src/main.rs", format!("use {lib}::Thing;\n\nfn main() {{}}\n").as_str())]);
        let labels: Vec<&str> = d["repo"]["stack"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Database"], "{lib}");
    }
}

/// every route of the first program: "VERB /path first-step" (no step: nothing after the path), sorted
fn route_heads(d: &Value) -> Vec<String> {
    sorted(procs(d)[0]["routes"].as_array().cloned().unwrap_or_default().iter().map(|r| {
        let first = r["steps"].as_array().and_then(|s| s.first()).map(|s| s["n"].as_str().unwrap().to_string()).unwrap_or_default();
        format!("{} {} {first}", r["verb"].as_str().unwrap(), r["path"].as_str().unwrap()).trim_end().to_string()
    }).collect())
}

#[test]
fn more_javascript_frameworks() {
    let handlers = "\nfunction list(req, res) {}\nfunction show(req, res) {}\nfunction create(req, res) {}\n";
    let js = |body: &str| route_heads(&analyze(&[("server.js", format!("{body}{handlers}").as_str())]));
    // Express: a router mounted under a path
    let r = js("const express = require('express')\nconst app = express()\nconst router = express.Router()\nrouter.get('/:id', show)\napp.use('/users', router)\napp.post('/login', create)\napp.listen(3000)\n");
    assert_eq!(r, ["GET /users/:id show()", "POST /login create()"]);
    // Koa: @koa/router with a prefix of its own, its routes handed to the app
    let r = js("const Koa = require('koa')\nconst Router = require('@koa/router')\nconst app = new Koa()\nconst router = new Router({ prefix: '/api' })\nrouter.get('/users', list)\nrouter.post('/users/:id', create)\napp.use(router.routes())\napp.listen(3000)\n");
    assert_eq!(r, ["GET /api/users list()", "POST /api/users/:id create()"]);
    // Fastify: routes on the instance
    let r = js("const fastify = require('fastify')({ logger: true })\nfastify.get('/users', list)\nfastify.post('/users', create)\nfastify.listen({ port: 3000 })\n");
    assert_eq!(r, ["GET /users list()", "POST /users create()"]);
    // Hono: a sub-app routed under a path; the routing call is no route of its own
    let r = js("import { Hono } from 'hono'\nconst app = new Hono()\nconst users = new Hono()\nusers.get('/:id', show)\nusers.post('/', create)\napp.route('/users', users)\napp.get('/health', list)\nexport default app\n");
    // (the root of a mounted router keeps its slash, as Express and FastAPI routes do)
    assert_eq!(r, ["GET /health list()", "GET /users/:id show()", "POST /users/ create()"]);
    // NestJS-style code outside JavaScript and TypeScript: a mount by `route`, an options object, a bare verb decorator change nothing
    let d = analyze(&[("main.py", "from flask import Flask\n\napp = Flask(__name__)\nusers = Flask(__name__)\napp.route('/users', users)\n\n@Post()\ndef create():\n    return 1\n\nif __name__ == \"__main__\":\n    app.run()\n")]);
    assert_eq!(route_heads(&d), ["ANY /users"]);
}

#[test]
fn fastify_plugins_take_the_prefix_they_are_registered_with() {
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"server.js\",\"dependencies\":{\"fastify\":\"4\"}}"),
        ("server.js", "const fastify = require('fastify')()\nconst users = require('./users')\nfastify.register(users, { prefix: '/users' })\nfastify.listen({ port: 3000 })\n"),
        ("users.js", "async function show(req, reply) {}\nmodule.exports = async function (app, opts) {\n  app.get('/:id', show)\n}\n"),
    ]);
    assert_eq!(route_heads(&d), ["GET /users/:id show()"]);
}

#[test]
fn nextjs_file_routes() {
    let d = analyze(&[
        ("package.json", r#"{"name": "site", "scripts": {"dev": "next dev"}, "dependencies": {"next": "14"}}"#),
        // pages router: the default export answers every verb, at the path of the file under pages/
        ("pages/api/users/[id].ts", "import type { NextApiRequest, NextApiResponse } from 'next'\nimport { find } from '../../../lib/db'\n\nexport default async function handler(req: NextApiRequest, res: NextApiResponse) {\n  res.json(find(req.query.id))\n}\n"),
        ("pages/api/health.js", "export default function (req, res) {\n  res.status(200).json(ping())\n}\n\nfunction ping() {\n  return 1\n}\n"),
        ("pages/api/index.js", "async function hello(req, res) {\n  res.end('hi')\n}\n\nexport default hello\n"),
        // app router: one route per function exported under a verb's name; a route group is no part of the path
        ("app/api/items/[id]/route.ts", "import { find } from '../../../../lib/db'\n\nexport async function GET(req: Request) {\n  return Response.json(find('1'))\n}\n\nexport async function DELETE(req: Request) {\n  return new Response(null)\n}\n\nfunction GETTER() {}\n"),
        ("app/(shop)/cart/route.js", "export const POST = async (req) => {\n  return new Response()\n}\n\nconst PUT = async () => {}\n"),
        ("app/blog/[...slug]/route.ts", "export function GET() {\n  return new Response()\n}\n"),
        // pages are not endpoints
        ("pages/about.jsx", "export default function About() {\n  return null\n}\n"),
        ("app/dashboard/page.tsx", "export default function Page() {\n  return null\n}\n"),
        ("app/layout.tsx", "export function GET() {\n  return null\n}\n"),
        ("lib/db.ts", "export function find(id: string) {\n  return id\n}\n"),
    ]);
    assert_eq!(route_heads(&d), [
        "ANY /api hello()",
        "ANY /api/health ping()",
        "ANY /api/users/{id} handler()",
        "DELETE /api/items/{id} DELETE()",
        "GET /api/items/{id} GET()",
        "GET /blog/{slug} GET()",
        "POST /cart POST()",
    ]);
    // the handler leads on to what it calls
    let r = procs(&d)[0]["routes"].as_array().unwrap().iter().find(|r| r["path"] == "/api/users/{id}" ).cloned().unwrap();
    let names: Vec<&str> = r["steps"].as_array().unwrap().iter().map(|s| s["n"].as_str().unwrap()).collect();
    assert_eq!(names, ["handler()", "find()"]);
    // the same places in another language are no routes
    let d = analyze(&[
        ("main.py", "import app.api.route\n\nif __name__ == \"__main__\":\n    app.api.route.GET()\n"),
        ("app/api/route.py", "def GET():\n    return 1\n"),
        ("pages/api/users.py", "def handler():\n    return 1\n"),
    ]);
    assert_eq!(route_heads(&d), Vec::<String>::new());
}

#[test]
fn nextjs_catch_all_pages_routes() {
    // only the last extension is the file's: catch-all and optional catch-all names are parameters, a dot in a folder stays
    let d = analyze(&[
        ("package.json", r#"{"name": "site", "scripts": {"dev": "next dev"}, "dependencies": {"next": "14"}}"#),
        ("pages/api/[...slug].ts", "export default function handler(req, res) {\n  res.end()\n}\n"),
        ("pages/api/auth/[...nextauth].ts", "export default function auth(req, res) {\n  res.end()\n}\n"),
        ("pages/api/files/[[...slug]].ts", "export default function optional(req, res) {\n  res.end()\n}\n"),
        ("pages/api/v1.0/ping.ts", "export default function ping(req, res) {\n  res.end()\n}\n"),
    ]);
    assert_eq!(route_heads(&d), ["ANY /api/auth/{nextauth} auth()", "ANY /api/files/{slug} optional()", "ANY /api/v1.0/ping ping()", "ANY /api/{slug} handler()"]);
}

#[test]
fn prisma_through_a_shared_module_or_a_service() {
    // a client declared in one file and imported by name in another
    let d = analyze(&[
        ("package.json", r#"{"name": "api", "main": "server.ts", "dependencies": {"@prisma/client": "5"}}"#),
        ("lib/prisma.ts", "import { PrismaClient } from '@prisma/client'\n\nexport const prisma = new PrismaClient()\n"),
        ("server.ts", "import { prisma } from './lib/prisma'\n\nasync function run(map, cache) {\n  map.get(k)\n  cache.delete(k)\n  await prisma.user.findMany()\n  await prisma.post.create({ data: {} })\n  await prisma.findMany()\n}\n\nrun()\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["reads User", "writes Post"]);
    // NestJS: a service that extends the client, injected where it is imported
    let d = analyze(&[
        ("package.json", r#"{"name": "api", "main": "src/main.ts", "dependencies": {"@prisma/client": "5"}}"#),
        ("src/prisma.service.ts", "import { Injectable } from '@nestjs/common'\nimport { PrismaClient } from '@prisma/client'\n\n@Injectable()\nexport class PrismaService extends PrismaClient {}\n"),
        ("src/main.ts", "import { PrismaService } from './prisma.service'\n\nexport class UsersService {\n  constructor(private readonly prisma: PrismaService) {}\n\n  list() {\n    return this.prisma.user.findMany()\n  }\n\n  remove(id: string) {\n    return this.prisma.user.delete({ where: { id } })\n  }\n}\n\nnew UsersService(null).list()\nnew UsersService(null).remove('1')\n"),
    ]);
    // (each method is reached once per call site: the labels, once each)
    let mut got = db_steps(&d, "");
    let mut seen = vec![];
    got.retain(|l| if seen.contains(l) { false } else { seen.push(l.clone()); true });
    assert_eq!(got, ["reads User", "writes User"]);
    // a plain object named prisma, not imported from the file that declares the client, is no handle
    let d = analyze(&[
        ("package.json", r#"{"name": "api", "main": "server.ts", "dependencies": {"@prisma/client": "5"}}"#),
        ("lib/prisma.ts", "import { PrismaClient } from '@prisma/client'\n\nexport const prisma = new PrismaClient()\n"),
        ("server.ts", "import { other } from './lib/prisma'\nconst prisma = { user: { findMany() { return [] } } }\n\nfunction run() {\n  return prisma.user.findMany()\n}\n\nrun()\n"),
    ]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
    // outside JavaScript and TypeScript a name imported from a file that declares a session is read as before
    let d = analyze(&[
        ("db.py", "from sqlalchemy.orm import Session\n\nsession: Session = Session()\n"),
        ("main.py", "from db import session\n\ndef run():\n    return session.query(1)\n\nif __name__ == \"__main__\":\n    run()\n"),
    ]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
}

#[test]
fn a_model_made_by_a_call_is_named_like_a_class() {
    // `const account = mongoose.model('Account', ..)` registers Account; a lower-case variable elsewhere is not a model
    let d = analyze(&[
        ("package.json", r#"{"name": "api", "main": "server.js", "dependencies": {"mongoose": "8"}}"#),
        ("models.js", "const mongoose = require('mongoose')\nconst account = mongoose.model('Account', new mongoose.Schema({}))\nmodule.exports = { account }\n"),
        ("server.js", "require('./models')\n\nasync function run(repo, account) {\n  const user = await repo.findOne()\n  user.save()\n  account.save()\n  account.deleteMany()\n}\n\nrun()\n"),
    ]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
}

#[test]
fn nestjs_controllers() {
    let d = analyze(&[
        ("package.json", "{\"name\":\"x\",\"main\":\"src/main.ts\",\"dependencies\":{\"@nestjs/core\":\"10\"}}"),
        ("src/main.ts", "import { NestFactory } from '@nestjs/core'\nimport { AppModule } from './app.module'\n\nasync function bootstrap() {\n  const app = await NestFactory.create(AppModule)\n  await app.listen(3000)\n}\nbootstrap()\n"),
        ("src/app.module.ts", "import { Module } from '@nestjs/common'\nimport { UsersController } from './users.controller'\n\n@Module({ controllers: [UsersController] })\nexport class AppModule {}\n"),
        ("src/users.controller.ts", "import { Controller, Get, Post } from '@nestjs/common'\n\n@Controller('users')\nexport class UsersController {\n  @Get(':id')\n  findOne() {\n    return {}\n  }\n\n  @Post()\n  create() {\n    return {}\n  }\n}\n"),
    ]);
    assert_eq!(route_heads(&d), ["GET /users/:id UsersController.findOne()", "POST /users UsersController.create()"]);
}

#[test]
fn javascript_libraries_are_catalogued() {
    for (lib, label) in [("hono", "Web framework"), ("@koa/router", "Web framework"), ("drizzle-orm/node-postgres", "Database"), ("@mikro-orm/core", "Database")] {
        let d = analyze(&[("server.js", format!("import x from '{lib}'\nx()\n").as_str())]);
        let labels: Vec<&str> = d["repo"]["stack"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap()).collect();
        assert_eq!(labels, [label], "{lib}");
    }
}

/// the "db" labels of the steps of a one-file program, each function's in the order they run
fn js_db(path: &str, src: &str) -> Vec<String> {
    db_steps(&analyze(&[(path, src)]), "")
}

#[test]
fn javascript_orm_prisma() {
    // the model is the member the call is made through; raw SQL reads or writes the database
    let got = js_db("main.ts", "import { PrismaClient } from '@prisma/client'\n\nconst prisma = new PrismaClient()\n\nasync function read() {\n  await prisma.user.findMany({ where: { active: true } })\n  await prisma.orderItem.count()\n  await prisma.$queryRaw`SELECT 1`\n}\n\nasync function write() {\n  await prisma.user.create({ data: {} })\n  await prisma.post.deleteMany()\n  await prisma.$executeRaw`DELETE FROM t`\n}\n\nread()\nwrite()\n");
    assert_eq!(got, ["reads User", "reads OrderItem", "reads database", "writes User", "writes Post", "writes database"]);
}

#[test]
fn javascript_orm_typeorm() {
    // a repository declared as Repository<User> does not say its entity where it is used: database; one made for the entity does
    let got = js_db("main.ts", "import { Entity, PrimaryGeneratedColumn, Repository, DataSource } from 'typeorm'\n\n@Entity()\nexport class User {\n  @PrimaryGeneratedColumn()\n  id: number\n}\n\nexport const AppDataSource = new DataSource({ type: 'postgres' })\n\nasync function read(repo: Repository<User>) {\n  await repo.find()\n  await AppDataSource.getRepository(User).findOneBy({ id: 1 })\n}\n\nasync function write(repo: Repository<User>) {\n  await repo.save({})\n  await AppDataSource.getRepository(User).softDelete(1)\n}\n\nasync function query(repo: Repository<User>) {\n  return repo.createQueryBuilder('u').where('u.id = :id', { id: 1 }).getMany()\n}\n\nread(null)\nwrite(null)\nquery(null)\n");
    assert_eq!(got, ["reads database", "reads User", "writes database", "writes User", "reads database"]);
    // an entity that extends BaseEntity is queried on its class
    let got = js_db("main.ts", "import { BaseEntity, Entity } from 'typeorm'\n\n@Entity()\nexport class Photo extends BaseEntity {}\n\nasync function run() {\n  await Photo.findBy({ id: 1 })\n  await Photo.insert({})\n}\n\nrun()\n");
    assert_eq!(got, ["reads Photo", "writes Photo"]);
}

#[test]
fn javascript_orm_sequelize() {
    // a class model and one made by define; an instance found by an awaited query writes its model
    let got = js_db("server.js", "const { Sequelize, Model, DataTypes } = require('sequelize')\nconst sequelize = new Sequelize('sqlite::memory:')\n\nclass User extends Model {}\nUser.init({ name: DataTypes.STRING }, { sequelize })\nconst Post = sequelize.define('Post', { title: DataTypes.STRING })\n\nasync function read() {\n  await User.findAll()\n  await Post.findByPk(1)\n}\n\nasync function write() {\n  await Post.bulkCreate([])\n  const user = await User.findOne({ where: { id: 1 } })\n  user.name = 'x'\n  await user.save()\n  await User.destroy({ where: { id: 2 } })\n}\n\nread()\nwrite()\n");
    assert_eq!(got, ["reads User", "reads Post", "writes Post", "reads User", "writes User"]);
}

#[test]
fn javascript_orm_mongoose() {
    let got = js_db("server.cjs", "const mongoose = require('mongoose')\nconst userSchema = new mongoose.Schema({ name: String })\nconst User = mongoose.model('User', userSchema)\nmodule.exports.Post = mongoose.model('Post', new mongoose.Schema({}))\n\nasync function read() {\n  await User.find({ active: true })\n  await User.countDocuments()\n}\n\nasync function write() {\n  const doc = new User({ name: 'x' })\n  await doc.save()\n  await User.findByIdAndUpdate(1, { name: 'y' })\n  await Post.deleteMany({})\n}\n\nread()\nwrite()\n");
    assert_eq!(got, ["reads User", "writes User", "writes Post"]);
    // TypeScript: the model function typed with the document's interface
    let got = js_db("main.ts", "import { Schema, model } from 'mongoose'\n\ninterface IUser { name: string }\nexport const User = model<IUser>('User', new Schema<IUser>({ name: String }))\n\nasync function run() {\n  await User.findById('1')\n  await User.insertMany([])\n}\n\nrun()\n");
    assert_eq!(got, ["reads User", "writes User"]);
}

#[test]
fn javascript_orm_drizzle() {
    // tables are values, not classes: the steps say database
    let got = js_db("main.ts", "import { drizzle } from 'drizzle-orm/node-postgres'\nimport { pgTable, serial, text } from 'drizzle-orm/pg-core'\nimport { eq } from 'drizzle-orm'\n\nexport const users = pgTable('users', { id: serial('id'), name: text('name') })\nconst db = drizzle(process.env.DATABASE_URL)\n\nasync function read() {\n  await db.select().from(users).where(eq(users.id, 1))\n  await db.query.users.findMany()\n}\n\nasync function write() {\n  await db.insert(users).values({ name: 'x' })\n  await db.update(users).set({ name: 'y' })\n  await db.delete(users).where(eq(users.id, 1))\n}\n\nread()\nwrite()\n");
    assert_eq!(got, ["reads database", "writes database"]);
}

#[test]
fn javascript_orm_knex() {
    // the handle is called with the table, then the query is chained; the SQL of raw is not read, so it adds no step
    let got = js_db("server.mjs", "import knex from 'knex'\n\nconst db = knex({ client: 'pg' })\n\nasync function read() {\n  await db('users').where({ id: 1 }).first()\n  await db.select('id').from('users')\n}\n\nasync function write() {\n  await db('users').insert({ name: 'x' })\n  await db('users').where({ id: 1 }).del()\n}\n\nasync function raw() {\n  await db.raw('select 1')\n}\n\nread()\nwrite()\nraw()\n");
    assert_eq!(got, ["reads database", "writes database"]);
    // the library required and called at once is the handle
    let got = js_db("server.js", "const knex = require('knex')({ client: 'pg' })\n\nasync function write() {\n  await knex('users').where('id', 1).update({ name: 'x' })\n}\n\nwrite()\n");
    assert_eq!(got, ["writes database"]);
}

#[test]
fn javascript_orm_mikro_orm() {
    let got = js_db("main.ts", "import { Entity, PrimaryKey, EntityManager } from '@mikro-orm/core'\n\n@Entity()\nexport class Book {\n  @PrimaryKey()\n  id!: number\n}\n\nasync function read(em: EntityManager) {\n  await em.find(Book, {})\n  await em.count(Book)\n}\n\nasync function write(em: EntityManager) {\n  em.persist(new Book())\n  await em.flush()\n  await em.nativeDelete(Book, { id: 1 })\n}\n\nread(null)\nwrite(null)\n");
    assert_eq!(got, ["reads Book", "writes Book", "writes database"]);
}

#[test]
fn javascript_orms_leave_plain_values_alone() {
    // a file that declares a real handle or model, then calls generic names on other values
    let preludes = [
        ("main.ts", "import { PrismaClient } from '@prisma/client'\nconst prisma = new PrismaClient()\n"),
        ("main.ts", "import { Entity, Repository } from 'typeorm'\n@Entity()\nexport class User {}\nexport let repo: Repository<User>\n"),
        ("server.js", "const { Model } = require('sequelize')\nclass User extends Model {}\n"),
        ("server.js", "const mongoose = require('mongoose')\nconst User = mongoose.model('User', new mongoose.Schema({}))\n"),
        ("main.ts", "import { drizzle } from 'drizzle-orm/node-postgres'\nconst db = drizzle('x')\n"),
        ("server.js", "import knex from 'knex'\nconst db = knex({})\n"),
        ("main.ts", "import { Entity, EntityManager } from '@mikro-orm/core'\n@Entity()\nexport class Book {}\nexport let em: EntityManager\n"),
    ];
    for (path, prelude) in preludes {
        let src = format!("{prelude}import axios from 'axios'\n\nfunction run(map, cache, arr, set, res, items, job, list, users, other) {{\n  map.get(k)\n  cache.delete(k)\n  arr.find(x => x.id === 1)\n  set.add(x)\n  axios.get(url)\n  res.json({{}})\n  items.update(x)\n  job.save()\n  list.count()\n  users.findMany()\n  other('users').first()\n}}\n\nrun()\n");
        assert_eq!(js_db(path, &src), Vec::<String>::new(), "{prelude}");
    }
}

#[test]
fn javascript_orm_reading_stays_in_javascript() {
    // an awaited query that hands back an instance is read the JavaScript way only: Python is read as before
    let d = analyze(&[
        ("Dockerfile", "FROM python\nCMD [\"uvicorn\", \"main:app\"]\n"),
        ("models.py", "from tortoise.models import Model\n\nclass User(Model):\n    pass\n"),
        ("main.py", "from fastapi import FastAPI\nfrom models import User\n\napp = FastAPI()\n\n@app.get(\"/x\")\nasync def x():\n    user = await User.get(id=1)\n    await user.save()\n"),
    ]);
    assert_eq!(db_steps(&d, "/x"), ["reads User"]);
    // a handle made by `new` is read the JavaScript way only
    let d = analyze(&[("public/index.php", "<?php\nfunction q() { $pdo = new PDO('sqlite::memory:'); return $pdo->query('SELECT 1'); }\nq();\n")]);
    assert_eq!(db_steps(&d, ""), Vec::<String>::new());
}

#[test]
fn php_frameworks() {
    // Laravel: a resource controller is five endpoints, each to its method
    let (d, r) = one_route_set(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../routes/web.php';\n"),
        ("routes/web.php", "<?php\nRoute::apiResource('photos', PhotoController::class);\n"),
        ("app/Http/Controllers/PhotoController.php", "<?php\nclass PhotoController {\n    public function index() {}\n    public function show($id) {}\n}\n"),
    ]);
    assert_eq!(r, ["DELETE /photos/{id}", "GET /photos", "GET /photos/{id}", "POST /photos", "PUT /photos/{id}"]);
    assert_eq!(route_first(&d, "/photos").as_deref(), Some("PhotoController.index()"));
    // Slim: a closure handler
    let (_, r) = one_route_set(&[("public/index.php", "<?php\n$app->get('/users', function ($req, $res) { return $res; });\n$app->run();\n")]);
    assert_eq!(r, ["GET /users"]);
}

#[test]
fn php_orms() {
    let steps = |files: &[(&str, &str)]| db_steps(&analyze(files), "");
    // Eloquent
    let got = steps(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../src/repo.php';\nfind_user();\nmake_user();\n"),
        ("src/repo.php", "<?php\nuse Illuminate\\Database\\Eloquent\\Model;\n\nclass User extends Model {}\n\nfunction find_user() { return User::where('id', 1)->first(); }\n\nfunction make_user() { return User::create(['name' => 'x']); }\n"),
    ]);
    assert_eq!(got, ["reads User", "writes User"]);
    // Doctrine: an entity marked by an attribute, a manager held in a property
    let got = steps(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../src/repo.php';\nsave_post();\nall_posts();\n"),
        ("src/repo.php", "<?php\nuse Doctrine\\ORM\\EntityManagerInterface;\n\n#[ORM\\Entity]\nclass Post {}\n\nclass Repo {\n    public function __construct(private EntityManagerInterface $em) {}\n    public function store($p) { $this->em->persist($p); $this->em->flush(); }\n    public function all() { return $this->em->getRepository(Post::class)->findAll(); }\n}\n\nfunction save_post() { (new Repo(null))->store(1); }\n\nfunction all_posts() { return (new Repo(null))->all(); }\n"),
    ]);
    assert!(got.contains(&"writes database".to_string()) && got.contains(&"reads Post".to_string()), "{got:?}");
    // PDO
    let got = steps(&[("public/index.php", "<?php\nfunction q(PDO $pdo) { return $pdo->query('SELECT 1'); }\nq(null);\n")]);
    assert_eq!(got, ["reads database"]);
}

// ---------------------------------------------------------------- found by running real repositories

#[test]
fn chained_routes_each_get_their_own_handler() {
    let d = analyze(&[("src/main.rs", "use axum::{routing::get, Router};\n\nfn main() {\n    let app = Router::new()\n        .route(\"/a\", get(first))\n        .route(\"/b\", get(second));\n}\n\nfn first() {}\n\nfn second() {}\n")]);
    assert_eq!(route_first(&d, "/a").as_deref(), Some("first()"));
    assert_eq!(route_first(&d, "/b").as_deref(), Some("second()"));
}

#[test]
fn django_regex_patterns_and_class_views() {
    let d = analyze(&[
        ("main.py", "import urls\nimport views\n\nif __name__ == \"__main__\":\n    urls.urlpatterns\n"),
        ("urls.py", "from django.conf.urls import url\nfrom views import FavoriteView\n\nurlpatterns = [url(r'^articles/(?P<slug>[-\\w]+)/favorite/?$', FavoriteView.as_view())]\n"),
        ("views.py", "class FavoriteView:\n    def post(self, request):\n        return 1\n\n    def delete(self, request):\n        return 2\n"),
    ]);
    assert_eq!(route_paths(&procs(&d)[0]), ["ANY /articles/{slug}/favorite"]);
    let names: Vec<String> = procs(&d)[0]["routes"][0]["steps"].as_array().unwrap().iter().map(|s| s["n"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["FavoriteView.post()", "FavoriteView.delete()"]); // every verb method the class defines
}

#[test]
fn a_session_returned_by_a_function_is_a_session() {
    let d = analyze(&[
        ("common/db.go", "package common\n\nimport \"github.com/jinzhu/gorm\"\n\nfunc GetDB() *gorm.DB { return nil }\n"),
        ("main.go", "package main\n\nimport \"app/common\"\n\nfunc main() { run() }\n\nfunc run() {\n\tdb := common.GetDB()\n\tdb.Where(\"id = ?\", 1).First(&x)\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["reads database"]);
}

#[test]
fn a_routes_file_nothing_requires_is_loaded_by_the_framework() {
    let d = analyze(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../vendor/autoload.php';\n$app = require_once __DIR__ . '/../bootstrap/app.php';\n"),
        ("routes/web.php", "<?php\nRoute::get('/home', 'HomeController@index');\n"),
        ("app/Http/Controllers/HomeController.php", "<?php\nclass HomeController {\n    public function index() {}\n}\n"),
    ]);
    let all: Vec<String> = procs(&d).iter().flat_map(route_paths).collect();
    assert_eq!(all, ["GET /home"]);
    assert_eq!(route_first(&d, "/home").as_deref(), Some("HomeController.index()"));
}

#[test]
fn a_session_held_in_a_field_declared_elsewhere() {
    let d = analyze(&[
        ("src/state.rs", "use sqlx::PgPool;\n\npub struct Ctx {\n    pub db: PgPool,\n}\n"),
        ("src/main.rs", "use sqlx::Row;\n\nfn main() { read(todo!()); }\n\nasync fn read(ctx: Ctx) {\n    sqlx::query(\"SELECT 1\").fetch_one(&ctx.db).await;\n}\n"),
    ]);
    assert_eq!(db_steps(&d, ""), ["reads database"]);
}

#[test]
fn a_controller_named_in_a_string() {
    let (d, r) = one_route_set(&[
        ("public/index.php", "<?php\nrequire __DIR__ . '/../routes/api.php';\n"),
        ("routes/api.php", "<?php\nRoute::resource('articles', 'ArticleController', ['except' => ['create']]);\n"),
        ("app/Http/Controllers/ArticleController.php", "<?php\nclass ArticleController {\n    public function index() {}\n}\n"),
    ]);
    assert!(r.contains(&"GET /articles".to_string()), "{r:?}");
    assert_eq!(route_first(&d, "/articles").as_deref(), Some("ArticleController.index()"));
}
