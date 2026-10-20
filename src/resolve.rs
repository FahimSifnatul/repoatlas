//! Imports -> files of the repo and outside libraries. The only per-language logic left: each language
//! spells "that other file" its own way (packages, relative paths, go.mod modules, crate paths, includes).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::lang::Resolver;
use crate::parse::File;
use crate::stdlib::PYTHON_STDLIB;

pub type Uses = (BTreeSet<String>, BTreeSet<(String, String)>);

pub fn dirname(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

pub fn basename(p: &str) -> &str {
    p.rfind('/').map(|i| &p[i + 1..]).unwrap_or(p)
}

/// posixpath.normpath for repo-relative paths: "a/./b/../c" -> "a/c" ("" for the root)
pub fn normpath(p: &str) -> String {
    let abs = p.starts_with('/');
    let mut out: Vec<&str> = vec![];
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !abs {
                    out.push("..");
                }
            }
            x => out.push(x),
        }
    }
    let s = out.join("/");
    if abs { format!("/{s}") } else { s }
}

pub fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        b.to_string()
    } else if b.is_empty() {
        a.to_string()
    } else {
        format!("{}/{}", a.trim_end_matches('/'), b)
    }
}

/// Read-only view of the repository that resolvers use.
pub struct Project {
    pub root: PathBuf,
    pub paths: HashSet<String>,
    pub dirs: HashSet<String>,
    /// manifest name -> [relative paths]; also "RUN", "README", "DEPS"
    pub man: BTreeMap<String, Vec<String>>,
    /// files that package.json / pyproject etc. name as entry points
    pub declared: Vec<String>,
    py_roots: Vec<String>,
    js_pkgs: BTreeMap<String, String>,
    js_dirnames: HashSet<String>,
    go_modules: BTreeMap<String, String>,
    go_packages: HashMap<String, Vec<String>>,
    rust_crates: BTreeMap<String, String>,
    jvm_fq: HashMap<String, String>,
    jvm_own: HashSet<String>,
    locs: HashMap<String, usize>,
}

impl Project {
    pub fn read(&self, rel: &str) -> String {
        std::fs::read(self.root.join(rel)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
    }

    pub fn new(root: &Path, files: &[File], man: BTreeMap<String, Vec<String>>) -> Project {
        let paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
        let mut dirs = HashSet::new();
        for p in &paths {
            let mut d = dirname(p);
            while !d.is_empty() {
                dirs.insert(d.to_string());
                d = dirname(d);
            }
        }
        let mut p = Project {
            root: root.to_path_buf(), paths, dirs, man, declared: vec![], py_roots: vec![], js_pkgs: BTreeMap::new(),
            js_dirnames: HashSet::new(), go_modules: BTreeMap::new(), go_packages: HashMap::new(), rust_crates: BTreeMap::new(),
            jvm_fq: HashMap::new(), jvm_own: HashSet::new(), locs: files.iter().map(|f| (f.path.clone(), f.loc)).collect(),
        };
        let used: HashSet<Resolver> = files.iter().map(|f| f.lang.resolver).collect();
        if used.contains(&Resolver::Python) {
            p.prepare_python();
        }
        if used.contains(&Resolver::Js) {
            p.prepare_js();
        }
        if used.contains(&Resolver::Go) {
            p.prepare_go(files);
        }
        if used.contains(&Resolver::Rust) {
            p.prepare_rust();
        }
        if used.contains(&Resolver::Jvm) {
            p.prepare_jvm(files);
        }
        p
    }

    pub fn resolve(&self, f: &File) -> Uses {
        match f.lang.resolver {
            Resolver::Python => self.resolve_python(f),
            Resolver::Js => self.resolve_js(&f.path, &f.imports),
            Resolver::Go => self.resolve_go(f),
            Resolver::Rust => self.resolve_rust(f),
            Resolver::Jvm => self.resolve_jvm(f),
            Resolver::C => self.resolve_c(f),
            Resolver::None => Default::default(),
        }
    }

    /// Local names that stand for a whole file of the repo: `import routes from './routes'`.
    /// Python: `from app.routes import items` binds items to routes/items.py, `from app.main import api` to main.py.
    pub fn bind(&self, f: &File) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = f.bindings
            .iter()
            .filter_map(|(name, spec)| self.resolve_js(&f.path, std::slice::from_ref(spec)).0.into_iter().next().map(|h| (name.clone(), h)))
            .collect();
        let d = dirname(&f.path);
        for imp in f.py_imports.iter().filter(|i| i.is_from) {
            let mut parts: Vec<&str> = if imp.module.is_empty() { vec![] } else { imp.module.split('.').collect() };
            let roots = if imp.level > 0 {
                let base: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
                let mut b = base[..base.len().saturating_sub(imp.level - 1)].to_vec();
                b.extend(parts);
                parts = b;
                vec![String::new()]
            } else {
                self.py_roots.clone()
            };
            let module = self.py_file(&parts, &roots);
            for n in imp.names.iter().filter(|n| *n != "*") {
                let mut sub = parts.clone();
                sub.push(n);
                if let Some(h) = self.py_file(&sub, &roots).or_else(|| module.clone()) {
                    out.push((n.clone(), h));
                }
            }
        }
        out
    }

    // ---------------------------------------------------------------- Python

    fn prepare_python(&mut self) {
        static SCRIPTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\[(?:project|tool\.poetry)\.scripts\]\s*\n(.*?)(?:\n\[|\z)").unwrap());
        static TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"=\s*"([\w.]+):[\w.]+""#).unwrap());
        let inits: HashSet<String> = self.man.get("__init__.py").into_iter().flatten().map(|p| dirname(p).to_string()).collect();
        let mut roots: BTreeSet<String> = [String::new()].into();
        for d in &inits {
            // the folder above the top-most package is an import root
            let mut top = d.as_str();
            while !top.is_empty() && inits.contains(dirname(top)) {
                top = dirname(top);
            }
            roots.insert(dirname(top).to_string());
        }
        for r in ["src", "lib"] {
            if self.dirs.contains(r) {
                roots.insert(r.into());
            }
        }
        let mut roots: Vec<String> = roots.into_iter().collect();
        roots.sort_by_key(|r| (!r.is_empty(), r.clone()));
        self.py_roots = roots;
        for rel in self.man.get("pyproject.toml").cloned().unwrap_or_default() {
            // console scripts are entry points
            let text = self.read(&rel);
            for sec in SCRIPTS.captures_iter(&text) {
                for m in TARGET.captures_iter(&sec[1]) {
                    let parts: Vec<&str> = m[1].split('.').collect();
                    if let Some(h) = self.py_file(&parts, &self.py_roots) {
                        self.declared.push(h);
                    }
                }
            }
        }
    }

    fn py_file(&self, parts: &[&str], roots: &[String]) -> Option<String> {
        for r in roots {
            let mut segs: Vec<&str> = if r.is_empty() { vec![] } else { vec![r.as_str()] };
            segs.extend_from_slice(parts);
            let base = segs.join("/");
            let cands = [
                (!base.is_empty()).then(|| format!("{base}.py")),
                Some(if base.is_empty() { "__init__.py".to_string() } else { format!("{base}/__init__.py") }),
            ];
            for c in cands.into_iter().flatten() {
                if self.paths.contains(&c) {
                    return Some(c);
                }
            }
        }
        None
    }

    fn resolve_python(&self, f: &File) -> Uses {
        let (mut uses, mut ext) = (BTreeSet::new(), BTreeSet::new());
        let d = dirname(&f.path);
        for imp in &f.py_imports {
            let mut parts: Vec<&str> = if imp.module.is_empty() { vec![] } else { imp.module.split('.').collect() };
            let roots: Vec<String>;
            if imp.level > 0 {
                // from . import x  /  from ..pkg import y
                let base: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
                let keep = base.len().saturating_sub(imp.level - 1);
                let mut b = base[..keep].to_vec();
                b.extend(parts);
                parts = b;
                roots = vec![String::new()];
            } else {
                roots = self.py_roots.clone();
            }
            let mut hits = vec![];
            if imp.is_from {
                for n in imp.names.iter().filter(|n| *n != "*") {
                    let mut p = parts.clone();
                    p.push(n);
                    if let Some(h) = self.py_file(&p, &roots) {
                        hits.push(h);
                    }
                }
                if hits.len() < imp.names.len() || imp.names.is_empty() {
                    if let Some(b) = self.py_file(&parts, &roots) {
                        hits.push(b);
                    }
                }
            } else {
                for k in (1..=parts.len()).rev() {
                    if let Some(h) = self.py_file(&parts[..k], &roots) {
                        hits.push(h);
                        break;
                    }
                }
            }
            if !hits.is_empty() {
                uses.extend(hits);
            } else if imp.level == 0 && !parts.is_empty() {
                if PYTHON_STDLIB.contains(&parts[0]) {
                    continue;
                }
                // a script importing a sibling file
                let sib = if d.is_empty() { None } else { self.py_file(&parts, &[d.to_string()]) };
                match sib {
                    Some(s) => {
                        uses.insert(s);
                    }
                    None => {
                        ext.insert((parts[0].to_string(), imp.module.clone()));
                    }
                }
            }
        }
        // importing a.b.c runs a/__init__.py and a/b/__init__.py first
        for h in uses.clone() {
            let mut pkg = dirname(&h).to_string();
            while !pkg.is_empty() && self.paths.contains(&format!("{pkg}/__init__.py")) && !d.starts_with(&pkg) {
                uses.insert(format!("{pkg}/__init__.py"));
                pkg = dirname(&pkg).to_string();
            }
        }
        (uses, ext)
    }

    // ---------------------------------------------------------------- JavaScript family

    fn prepare_js(&mut self) {
        static SCRIPT: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?:node|ts-node|tsx|nodemon|bun|deno run)\s+([\w./-]+\.(?:js|ts|mjs|cjs))").unwrap());
        self.js_dirnames = self.dirs.iter().filter(|x| x.matches('/').count() <= 1).map(|x| basename(x).to_string()).collect();
        for rel in self.man.get("package.json").cloned().unwrap_or_default() {
            let Ok(serde_json::Value::Object(j)) = serde_json::from_str::<serde_json::Value>(&self.read(&rel)) else { continue };
            let d = dirname(&rel).to_string();
            if let Some(serde_json::Value::String(n)) = j.get("name") {
                self.js_pkgs.insert(n.clone(), d.clone());
            }
            let mut cands: Vec<String> = vec![];
            if let Some(serde_json::Value::String(m)) = j.get("main") {
                cands.push(m.clone());
            }
            match j.get("bin") {
                Some(serde_json::Value::String(b)) => cands.push(b.clone()),
                Some(serde_json::Value::Object(b)) => cands.extend(b.values().filter_map(|v| v.as_str().map(String::from))),
                _ => {}
            }
            if let Some(serde_json::Value::Object(s)) = j.get("scripts") {
                for v in s.values() {
                    if let Some(c) = v.as_str().and_then(|v| SCRIPT.captures(v)) {
                        cands.push(c[1].to_string());
                    }
                }
            }
            for c in cands {
                if let Some(h) = self.js_file(&normpath(&join(&d, &c))) {
                    self.declared.push(h);
                }
            }
        }
    }

    /// './a' -> a.ts / a.js / a/index.js ...; also './a.js' written for an a.ts source.
    fn js_file(&self, base: &str) -> Option<String> {
        const EXTS: &[&str] = &["", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".vue", ".svelte"];
        static JS_EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*)\.(?:m|c)?jsx?$").unwrap());
        let base = base.trim_matches('/');
        let mut cands = vec![base.to_string()];
        if let Some(c) = JS_EXT.captures(base) {
            cands.push(format!("{}.ts", &c[1]));
            cands.push(format!("{}.tsx", &c[1]));
        }
        for c in &cands {
            for e in EXTS {
                if self.paths.contains(&format!("{c}{e}")) {
                    return Some(format!("{c}{e}"));
                }
            }
            for e in &EXTS[1..] {
                let i = if c.is_empty() { format!("index{e}") } else { format!("{c}/index{e}") };
                if self.paths.contains(&i) {
                    return Some(i);
                }
            }
        }
        None
    }

    fn resolve_js(&self, path: &str, specs: &[String]) -> Uses {
        const NODE_BUILTIN: &[&str] = &["fs", "path", "http", "https", "os", "util", "crypto", "events", "stream", "url",
            "child_process", "zlib", "assert", "buffer", "net", "dns", "readline", "cluster", "tls", "querystring", "timers",
            "process", "module", "worker_threads", "perf_hooks"];
        let (mut uses, mut ext) = (BTreeSet::new(), BTreeSet::new());
        for s in specs {
            if s.starts_with('.') {
                if let Some(h) = self.js_file(&normpath(&join(dirname(path), s))) {
                    uses.insert(h);
                }
                continue;
            }
            let mut hit = None;
            if s.starts_with("@/") || s.starts_with("~/") || s.starts_with("#/") {
                // path alias: look in every enclosing folder's src/ (monorepo friendly)
                let mut anc = vec![];
                let mut d = dirname(path);
                loop {
                    anc.push(d);
                    if d.is_empty() {
                        break;
                    }
                    d = dirname(d);
                }
                hit = anc.iter().flat_map(|a| ["src", "app", ""].map(|r| join(&join(a, r), &s[2..]))).find_map(|c| self.js_file(&c));
                if hit.is_none() {
                    continue; // an alias we cannot resolve is not an outside library
                }
            } else {
                for (name, d) in &self.js_pkgs {
                    if s == name || s.starts_with(&format!("{name}/")) {
                        let rest = s.get(name.len() + 1..).unwrap_or("");
                        let cands = if rest.is_empty() { vec![format!("{d}/src/index"), format!("{d}/index")] } else { vec![format!("{d}/{rest}"), format!("{d}/src/{rest}")] };
                        hit = cands.iter().find_map(|c| self.js_file(c));
                        break;
                    }
                }
                if hit.is_none() && !s.starts_with('@') && self.js_dirnames.contains(s.split('/').next().unwrap_or("")) {
                    hit = ["src", ""].iter().find_map(|r| self.js_file(&join(r, s)));
                }
            }
            match hit {
                Some(h) => {
                    uses.insert(h);
                }
                None => {
                    if s.starts_with("node:") || NODE_BUILTIN.contains(&s.as_str()) {
                        continue;
                    }
                    let parts: Vec<&str> = s.split('/').collect();
                    let name = if s.starts_with('@') { parts[..parts.len().min(2)].join("/") } else { parts[0].to_string() };
                    ext.insert((name, s.clone()));
                }
            }
        }
        (uses, ext)
    }

    // ---------------------------------------------------------------- Go

    fn prepare_go(&mut self, files: &[File]) {
        static MODULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^module\s+(\S+)").unwrap());
        for rel in self.man.get("go.mod").cloned().unwrap_or_default() {
            if let Some(m) = MODULE.captures(&self.read(&rel)) {
                self.go_modules.insert(m[1].to_string(), dirname(&rel).to_string());
            }
        }
        for f in files.iter().filter(|f| f.lang.resolver == Resolver::Go && !f.path.ends_with("_test.go")) {
            self.go_packages.entry(dirname(&f.path).to_string()).or_default().push(f.path.clone());
        }
    }

    fn resolve_go(&self, f: &File) -> Uses {
        let (mut uses, mut ext) = (BTreeSet::new(), BTreeSet::new());
        for imp in &f.imports {
            let mut hit = None;
            for (m, d) in &self.go_modules {
                if imp == m || imp.starts_with(&format!("{m}/")) {
                    let target = if imp == m { d.clone() } else { normpath(&join(d, &imp[m.len() + 1..])) };
                    let fs: Vec<&String> = self.go_packages.get(&target).into_iter().flatten().filter(|p| **p != f.path).collect();
                    if !fs.is_empty() {
                        let base = basename(&target);
                        hit = fs.iter().find(|p| basename(p) == format!("{base}.go")).copied()
                            .or_else(|| fs.iter().max_by_key(|p| (self.locs.get(p.as_str()).copied().unwrap_or(0), std::cmp::Reverse(p.as_str()))).copied())
                            .cloned();
                    }
                    break;
                }
            }
            match hit {
                Some(h) => {
                    uses.insert(h);
                }
                // github.com/x/y is outside; "fmt" is the standard library
                None if imp.split('/').next().unwrap_or("").contains('.') => {
                    ext.insert((imp.split('/').take(3).collect::<Vec<_>>().join("/"), imp.clone()));
                }
                None => {}
            }
        }
        (uses, ext)
    }

    // ---------------------------------------------------------------- Rust

    fn prepare_rust(&mut self) {
        static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\s*name\s*=\s*"([^"]+)""#).unwrap());
        for rel in self.man.get("Cargo.toml").cloned().unwrap_or_default() {
            if let Some(m) = NAME.captures(&self.read(&rel)) {
                self.rust_crates.insert(m[1].replace('-', "_"), dirname(&rel).to_string());
            }
        }
    }

    fn resolve_rust(&self, f: &File) -> Uses {
        let (mut uses, mut ext) = (BTreeSet::new(), BTreeSet::new());
        let path = f.path.as_str();
        let d = dirname(path);
        let stem = basename(path).trim_end_matches(".rs");
        let moddir = if ["mod", "lib", "main"].contains(&stem) { d.to_string() } else { join(d, stem) };
        let dparts: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
        let src = match dparts.iter().rposition(|p| *p == "src") {
            Some(i) => dparts[..=i].join("/"),
            None => String::new(),
        };
        let find = |base: &str, rest: &[&str]| -> Option<String> {
            for k in (1..=rest.len()).rev() {
                let b = join(base, &rest[..k].join("/"));
                for c in [format!("{b}.rs"), format!("{b}/mod.rs")] {
                    if self.paths.contains(&c) && c != path {
                        return Some(c);
                    }
                }
            }
            None
        };
        for m in &f.mods {
            if let Some(h) = find(&moddir, &[m.as_str()]) {
                uses.insert(h);
            }
        }
        for u in &f.imports {
            let parts: Vec<&str> = u.split("::").filter(|x| !x.is_empty()).collect();
            if parts.is_empty() || ["std", "core", "alloc"].contains(&parts[0]) {
                continue;
            }
            let head = parts[0];
            let (base, rest): (String, Vec<&str>) = if head == "crate" {
                (src.clone(), parts[1..].to_vec())
            } else if head == "self" {
                (moddir.clone(), parts[1..].to_vec())
            } else if head == "super" {
                let mut base = moddir.clone();
                let mut ps = parts.as_slice();
                while ps.first() == Some(&"super") {
                    base = dirname(&base).to_string();
                    ps = &ps[1..];
                }
                (base, ps.to_vec())
            } else if f.mods.iter().any(|m| m == head) {
                (moddir.clone(), parts.clone())
            } else if let Some(c) = self.rust_crates.get(head) {
                (join(c, "src"), parts[1..].to_vec())
            } else {
                ext.insert((head.to_string(), head.to_string()));
                continue;
            };
            if !rest.is_empty() {
                if let Some(h) = find(&base, &rest) {
                    uses.insert(h);
                }
            }
        }
        (uses, ext)
    }

    // ---------------------------------------------------------------- Java / Kotlin

    fn prepare_jvm(&mut self, files: &[File]) {
        for f in files.iter().filter(|f| f.lang.resolver == Resolver::Jvm && !f.pkg.is_empty()) {
            let stem = basename(&f.path).rsplit_once('.').map(|x| x.0).unwrap_or("");
            self.jvm_fq.insert(format!("{}.{}", f.pkg, stem), f.path.clone());
        }
        self.jvm_own = self.jvm_fq.keys().map(|k| k.split('.').take(2).collect::<Vec<_>>().join(".")).collect();
    }

    fn resolve_jvm(&self, f: &File) -> Uses {
        let (mut uses, mut ext) = (BTreeSet::new(), BTreeSet::new());
        for imp in &f.imports {
            let parts: Vec<&str> = imp.split('.').collect();
            let hit = (2..=parts.len()).rev().find_map(|k| self.jvm_fq.get(&parts[..k].join(".")));
            match hit {
                Some(h) => {
                    if *h != f.path {
                        uses.insert(h.clone());
                    }
                }
                None => {
                    let own = parts.iter().take(2).copied().collect::<Vec<_>>().join(".");
                    if !self.jvm_own.contains(&own) && !["java.", "kotlin.", "android."].iter().any(|p| imp.starts_with(p)) {
                        // jakarta.persistence, not .Entity
                        let name: Vec<&str> = parts.iter().take(3).copied().filter(|x| !x.starts_with(|c: char| c.is_uppercase())).collect();
                        ext.insert((name.join("."), imp.clone()));
                    }
                }
            }
        }
        (uses, ext)
    }

    // ---------------------------------------------------------------- C / C++

    fn resolve_c(&self, f: &File) -> Uses {
        let mut uses = BTreeSet::new();
        for inc in &f.imports {
            for base in [dirname(&f.path), "include", "src", ""] {
                let c = normpath(&join(base, inc));
                if self.paths.contains(&c) && c != f.path {
                    uses.insert(c);
                    break;
                }
            }
        }
        (uses, BTreeSet::new())
    }
}
