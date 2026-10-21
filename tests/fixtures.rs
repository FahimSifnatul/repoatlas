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

// ---------------------------------------------------------------- journeys: every step, in order

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
