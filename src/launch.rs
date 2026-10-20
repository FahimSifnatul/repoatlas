//! How the project is started: commands written down in Dockerfiles, compose files, Procfiles, package.json,
//! pyproject, Makefiles and service configs, and commands a program of the repo runs itself.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use crate::finder::{repo_path, Finder};
use crate::parse::File;
use crate::resolve::{basename, dirname, join, normpath, Project};
use crate::tables::{CHDIR, IO_RES, LAUNCH_FILES, STRING, WRAPPERS};

#[derive(Clone, Debug, Default)]
pub struct Launch {
    pub name: String,
    pub cmd: String,
    pub file: String,
    /// the folder it runs in: a repo path, a path inside a container (starts with "/"), or None when unknown
    pub cwd: Option<String>,
    /// page files a web server config hands requests to
    pub serves: String,
    pub rank: i32,
    /// the image a config was copied into
    pub parent: Option<usize>,
    /// compose / k8s: the Dockerfile a service is built from (Some(None): a service with no build of its own)
    pub image: Option<Option<String>>,
    /// Dockerfile: the repo paths it copies in
    pub copied: Option<Vec<String>>,
    pub pages: Vec<String>,
    pub targets: Vec<String>,
    /// the launch that started this one
    pub via: Option<usize>,
    removed: bool,
}

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

fn indent(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    v.into_iter().filter(|x| seen.insert(x.clone())).collect()
}

/// A container path -> the repo path a Dockerfile COPY or a compose volume put there.
pub struct Where {
    copies: Vec<(String, String)>,
}

impl Where {
    pub fn get(&self, path: &str) -> Option<String> {
        let path = normpath(path);
        let mut best: Option<&(String, String)> = None;
        for c in &self.copies {
            let under = path == c.0 || path.starts_with(&format!("{}/", c.0.trim_end_matches('/')));
            if under && best.is_none_or(|b| c.0.len() > b.0.len()) {
                best = Some(c);
            }
        }
        if let Some((dst, src)) = best {
            return Some(repo_path(src, path[dst.len().min(path.len())..].trim_start_matches('/')));
        }
        for (dst, src) in &self.copies {
            // a folder above what was copied: /app/api holds /app/api/src <- api/src, so it is api
            if dst.starts_with(&format!("{}/", path.trim_end_matches('/'))) {
                let tail = dst[path.len()..].trim_matches('/');
                if !tail.is_empty() && (src == tail || src.ends_with(&format!("/{tail}"))) {
                    return Some(src[..src.len() - tail.len()].trim_end_matches('/').to_string());
                }
            }
        }
        None
    }
}

/// Commands written down in launch files, and the container-path lookup they imply.
pub fn read_launch_files(project: &Project) -> (Vec<Launch>, Where) {
    let mut copies: Vec<(String, String)> = vec![];
    let mut raw: Vec<Launch> = vec![];
    let mut baked: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut conf_of: Vec<(String, Vec<usize>)> = vec![];
    // (folder, compose service) -> (working folder, image it is built from)
    let mut services: HashMap<(String, String), (Option<String>, Option<String>)> = HashMap::new();
    let exists = |rel: &str| project.root.join(rel).exists();
    let rank = |kind: &str| LAUNCH_FILES.iter().position(|(_, k)| *k == kind).unwrap_or(0) as i32;
    let add = |raw: &mut Vec<Launch>, name: &str, cmd: &str, rel: &str, cwd: Option<String>, kind: &str, serves: String| -> usize {
        raw.push(Launch { name: name.into(), cmd: cmd.trim().into(), file: rel.into(), cwd, serves, rank: rank(kind), ..Default::default() });
        raw.len() - 1
    };

    for rel in project.man.get("RUN").cloned().unwrap_or_default() {
        let text = project.read(&rel);
        let mut d = dirname(&rel).to_string();
        let fname = basename(&rel).to_string();
        let Some(kind) = LAUNCH_FILES.iter().find(|(re, _)| re.is_match(&fname)).map(|(_, k)| *k) else { continue };
        let stem = fname.rsplit_once('.').map(|x| x.0).unwrap_or(&fname).to_string();
        match kind {
            "docker" => {
                static STEP: LazyLock<Regex> = LazyLock::new(|| rx(r"(?mi)^\s*(WORKDIR|COPY|ADD|CMD|ENTRYPOINT)\s+(.+?)\s*$"));
                static NAME: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)^dockerfile\.?|\.?(?:docker|dockerfile)$"));
                let steps: Vec<(String, String)> = STEP.captures_iter(&text).map(|c| (c[1].to_uppercase(), c[2].to_string())).collect();
                let srcs_all: Vec<String> = steps
                    .iter()
                    .filter(|(kw, arg)| (kw == "COPY" || kw == "ADD") && !arg.contains("--from"))
                    .flat_map(|(_, arg)| {
                        let ws: Vec<&str> = arg.split_whitespace().collect();
                        ws[..ws.len().saturating_sub(1)].iter().filter(|w| !w.starts_with("--")).map(|w| w.to_string()).collect::<Vec<_>>()
                    })
                    .collect();
                if srcs_all.iter().any(|s| !exists(&repo_path(&d, s)) && exists(&repo_path("", s))) {
                    d = String::new(); // built from the repo root (docker build -f sub/Dockerfile .), not from its own folder
                }
                let mut name = NAME.replace_all(&fname, "").to_string();
                if name.is_empty() {
                    name = basename(dirname(&rel)).to_string();
                }
                if name.is_empty() {
                    name = "container".into();
                }
                let mut wd = "/".to_string();
                let mut run: HashMap<String, (String, String)> = HashMap::new();
                let mut copied = vec![];
                for (kw, arg) in &steps {
                    if kw == "WORKDIR" {
                        wd = normpath(&join(&wd, arg));
                        if wd.is_empty() {
                            wd = "/".into();
                        }
                    } else if kw == "CMD" || kw == "ENTRYPOINT" {
                        run.insert(kw.clone(), (arg.clone(), wd.clone())); // the last of each counts; CMD becomes the ENTRYPOINT's arguments
                    } else if !arg.contains("--from") {
                        let ws: Vec<&str> = arg.split_whitespace().filter(|w| !w.starts_with("--")).collect();
                        let Some((dst, srcs)) = ws.split_last() else { continue };
                        let into = srcs.len() > 1 || dst.ends_with('/');
                        let dst = join(&wd, dst);
                        for s in srcs {
                            let src = repo_path(&d, s);
                            copied.push(src.clone());
                            let folder = src.is_empty() || project.dirs.contains(&src);
                            let to = if into && !folder { normpath(&join(&dst, basename(s))) } else { dst.clone() };
                            copies.push((to, src));
                        }
                    }
                }
                let mut mine = vec![];
                if !run.is_empty() {
                    let cmd: Vec<&str> = ["ENTRYPOINT", "CMD"].iter().filter_map(|k| run.get(*k).map(|x| x.0.as_str())).collect();
                    let cwd = run.get("ENTRYPOINT").or(run.get("CMD")).map(|x| x.1.clone());
                    let i = add(&mut raw, &name, &cmd.join(" "), &rel, cwd, kind, String::new());
                    raw[i].copied = Some(copied.clone());
                    mine.push(i);
                }
                for src in copied {
                    // configs baked into the image: its server pages and supervised programs
                    baked.entry(src).or_default().extend(mine.iter().copied());
                }
            }
            "yaml" => read_yaml(&text, &rel, &d, &stem, kind, &mut raw, &mut copies, &mut services, &add),
            "procfile" => {
                static LINE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^([\w-]+)\s*:\s*(.+)$"));
                for c in LINE.captures_iter(&text) {
                    add(&mut raw, &c[1], &c[2], &rel, Some(d.clone()), kind, String::new());
                }
            }
            "make" => {
                static VAR: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^(\w+)\s*[:?+]?=\s*(.*)$"));
                static REF: LazyLock<Regex> = LazyLock::new(|| rx(r"\$[({](\w+)[)}]"));
                static TARGET: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^([\w.-]+)\s*:(?:[^=\n][^\n]*)?\n((?:\t[^\n]*(?:\n|$))+)"));
                let var: HashMap<String, String> = VAR.captures_iter(&text).map(|c| (c[1].to_string(), c[2].to_string())).collect();
                let sub = |s: &str| REF.replace_all(s, |c: &regex::Captures| var.get(&c[1]).cloned().unwrap_or_else(|| format!("${}", &c[1]))).to_string();
                for c in TARGET.captures_iter(&text) {
                    for line in c[2].lines() {
                        let cmd = sub(&sub(line));
                        add(&mut raw, &c[1], cmd.trim().trim_start_matches(['@', '-', '+', ' ']), &rel, Some(d.clone()), kind, String::new());
                    }
                }
            }
            "ini" => {
                // supervisor [program:x] command=, systemd ExecStart=; nginx root, Apache DocumentRoot
                static SEC: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*\[(?:\w+:)?([^\]]+)\]"));
                static CMD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:command|ExecStart)\s*=\s*[-@+!:]*(.+)$"));
                static ROOT: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?mi)^\s*(?:root|DocumentRoot)\s+["']?([^\s;"']+)"#));
                static INDEX: LazyLock<Regex> = LazyLock::new(|| rx(r"(?mi)^\s*(?:index|DirectoryIndex)\s+([^;\n]+)"));
                let mut sec = stem.clone();
                let mut mine = vec![];
                for line in text.lines() {
                    if let Some(h) = SEC.captures(line) {
                        sec = h[1].trim().to_string();
                    }
                    if let Some(c) = CMD.captures(line) {
                        mine.push(add(&mut raw, &sec, &c[1], &rel, None, kind, String::new()));
                    }
                }
                for root in ROOT.captures_iter(&text) {
                    let mut pages: Vec<String> = INDEX.captures(&text).map(|c| c[1].split_whitespace().map(String::from).collect()).unwrap_or_default();
                    pages.extend(["index.php", "index.html", "index.js"].map(String::from));
                    let serves: Vec<String> = dedup(pages).iter().map(|x| join(&root[1], x)).collect();
                    mine.push(add(&mut raw, &stem, "", &rel, None, kind, serves.join(" ")));
                }
                conf_of.push((rel.clone(), mine));
            }
            "json" => {
                // package.json / composer.json / deno.json: "scripts", "tasks", "bin"
                static HOOK: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:(?:pre|post)[A-Z_:-]|(?:pre|post)(?:install|publish|pack|version|test))"));
                let Ok(serde_json::Value::Object(j)) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                for key in ["scripts", "tasks", "bin"] {
                    let items: Vec<(String, serde_json::Value)> = match j.get(key) {
                        Some(serde_json::Value::String(s)) => {
                            let n = j.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()).unwrap_or(key);
                            vec![(n.to_string(), serde_json::Value::String(s.clone()))]
                        }
                        Some(serde_json::Value::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                        _ => vec![],
                    };
                    for (n, c) in items {
                        if HOOK.is_match(&n) {
                            continue;
                        }
                        let cmd = match &c {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(a) => a.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(" "),
                            other => other.to_string(),
                        };
                        add(&mut raw, &n, &cmd, &rel, Some(d.clone()), kind, String::new());
                    }
                }
            }
            "toml" => {
                // [project.scripts], [tool.poetry.scripts] ...
                static HEAD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\[[\w.-]*scripts\]\s*$"));
                static ITEM: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*["']?([\w.-]+)["']?\s*=\s*["']([^"'\n]+)"#));
                let mut on = false;
                for line in text.lines() {
                    if line.starts_with('[') {
                        on = HEAD.is_match(line);
                        continue;
                    }
                    if let Some(c) = ITEM.captures(line).filter(|_| on) {
                        add(&mut raw, &c[1], &c[2], &rel, Some(d.clone()), kind, String::new());
                    }
                }
            }
            _ => {}
        }
    }

    for L in raw.iter_mut() {
        // compose.override.yml adds a command to a service compose.yml builds: same service, same folder
        if let (Some(None), Some(base)) = (&L.image, services.get(&(dirname(&L.file).to_string(), L.name.clone()))) {
            if L.cwd.is_none() {
                L.cwd = base.0.clone();
            }
            L.image = Some(base.1.clone());
        }
    }
    for (conf, items) in &conf_of {
        // a config copied into an image runs inside that image
        let hosts = baked.get(conf).cloned().unwrap_or_default();
        for &i in items {
            if !raw[i].serves.is_empty() && !hosts.is_empty() {
                // its web root: the image's own command serves those pages
                let s = raw[i].serves.clone();
                for &h in &hosts {
                    raw[h].serves = format!("{} {}", raw[h].serves, s).trim().to_string();
                }
                raw[i].removed = true;
            } else if let Some(&h) = hosts.first() {
                raw[i].parent = Some(h);
            }
        }
    }
    // drop the removed ones, keeping `parent` pointing at the same launch
    let keep: Vec<usize> = (0..raw.len()).filter(|&i| !raw[i].removed).collect();
    let new_index: HashMap<usize, usize> = keep.iter().enumerate().map(|(n, &o)| (o, n)).collect();
    let mut out: Vec<Launch> = keep.iter().map(|&i| raw[i].clone()).collect();
    for L in out.iter_mut() {
        L.parent = L.parent.and_then(|p| new_index.get(&p).copied());
    }
    (out, Where { copies })
}

type Add<'a> = dyn Fn(&mut Vec<Launch>, &str, &str, &str, Option<String>, &str, String) -> usize + 'a;

/// compose services, k8s containers, CI jobs: `command:` and friends, named by their block
#[allow(clippy::too_many_arguments)]
fn read_yaml(text: &str, rel: &str, d: &str, stem: &str, kind: &str, raw: &mut Vec<Launch>, copies: &mut Vec<(String, String)>,
             services: &mut HashMap<(String, String), (Option<String>, Option<String>)>, add: &Add) {
    static MERGE: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*<<\s*:\s*\*([\w.-]+)"));
    static CTX: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*(?:context|build)\s*:\s*["']?([^\s"'{]+)"#));
    static DF: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*dockerfile\s*:\s*["']?([^\s"']+)"#));
    static WD: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*working_dir\s*:\s*["']?([^\s"']+)"#));
    static BUILD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:build|context|working_dir)\s*:"));
    static KEY: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*[\w.-]+\s*:\s*$"));
    static VOL: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*-\s*["']?(\.{1,2}(?:/[^:"']*)?)\s*:\s*(/[^:"'\s]*)"#));
    static CMD: LazyLock<Regex> = LazyLock::new(|| rx(r"^(\s*)(?:-\s+)?(command|entrypoint|cmd|run|start|script|exec)\s*:\s*(.*)$"));
    static ITEM: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*-\s*"));
    static OWNER: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*(?:-\s+)?(?:name\s*:\s*)?["']?([\w.-]+)["']?\s*:?\s*$"#));
    let lines: Vec<&str> = text.lines().collect();

    // the lines of the block that opens at line j, with any `<<: *anchor` it merges in
    fn block<'a>(lines: &[&'a str], j: usize, depth: usize) -> Vec<&'a str> {
        let top = indent(lines[j]);
        let mut out = vec![];
        for b in &lines[j + 1..] {
            if !b.trim().is_empty() && indent(b) <= top {
                break;
            }
            out.push(*b);
            if let Some(m) = MERGE.captures(b).filter(|_| depth < 4) {
                let anchor = Regex::new(&format!(r"&{}\b", regex::escape(&m[1]))).unwrap();
                if let Some(a) = lines.iter().position(|l| anchor.is_match(l)).filter(|&a| a != j) {
                    out.extend(block(lines, a, depth + 1));
                }
            }
        }
        out
    }
    // built from this repo: the Dockerfile it is built from
    let image_of = |blk: &[&str]| -> Option<String> {
        let ctx = blk.iter().find_map(|b| CTX.captures(b))?;
        let c = repo_path(d, &ctx[1]);
        Some(match blk.iter().find_map(|b| DF.captures(b)) {
            Some(df) => repo_path(&c, &df[1]),
            None => repo_path(&c, "Dockerfile"),
        })
    };
    let folder_of = |blk: &[&str]| blk.iter().find_map(|b| WD.captures(b).map(|c| c[1].to_string()));

    for (i, line) in lines.iter().enumerate() {
        if BUILD.is_match(line) {
            // remember how each service is built
            let j = (0..i).rev().find(|&j| {
                !lines[j].trim().is_empty() && indent(lines[j]) < indent(line) && KEY.is_match(lines[j])
                    && lines[j].trim().trim_end_matches(':').trim() != "build"
            });
            if let Some(j) = j {
                let key = (d.to_string(), lines[j].trim().trim_end_matches(':').trim().to_string());
                let blk = block(&lines, j, 0);
                services.entry(key).or_insert_with(|| (folder_of(&blk), image_of(&blk)));
            }
        }
        if let Some(v) = VOL.captures(line) {
            copies.push((normpath(&v[2]), repo_path(d, &v[1])));
        }
        let Some(m) = CMD.captures(line) else { continue };
        let ind = m[1].len();
        let mut val = m[3].trim().to_string();
        if ["", "|", ">", "|-", ">-"].contains(&val.as_str()) {
            // the command continues on the lines below
            let mut vals = vec![];
            for nxt in &lines[i + 1..] {
                let k = indent(nxt);
                if !nxt.trim().is_empty() && (k < ind || (k == ind && !nxt.trim_start().starts_with("- "))) {
                    break;
                }
                vals.push(ITEM.replace(nxt, "").trim().to_string());
            }
            val = vals.into_iter().filter(|v| !v.is_empty()).collect::<Vec<_>>().join(" ");
        }
        let (mut name, mut wd, mut image) = (stem.to_string(), None, None);
        for j in (0..i).rev() {
            // the block it belongs to: a compose service, a container, a job
            if let Some(pm) = OWNER.captures(lines[j]).filter(|_| !lines[j].trim().is_empty() && indent(lines[j]) < ind) {
                name = pm[1].to_string();
                let blk = block(&lines, j, 0);
                wd = folder_of(&blk);
                image = image_of(&blk);
                break;
            }
        }
        let k = add(raw, &name, &val, rel, wd, kind, String::new());
        raw[k].image = Some(image);
    }
}

/// Names of what a project needs to run (not its dev tools), from its dependency file.
pub fn dependencies(rel: &str, text: &str) -> Vec<String> {
    static GO: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*(?:require\s+)?([\w.-]+\.[\w.-]+/[\w./-]+)\s+v\d"));
    static GEM: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?m)^\s*gem\s+['"]([\w-]+)"#));
    static REQ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*([A-Za-z][\w.-]*)"));
    static SECTION: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\[([^\]]+)\]\s*$"));
    static DEV: LazyLock<Regex> = LazyLock::new(|| rx(r"dev|test|lint|doc|group"));
    static QUOTED: LazyLock<Regex> = LazyLock::new(|| rx(r#"["']([A-Za-z][\w.-]*)(?:\[[^\]]*\])?(?:[<>=~!;,@][^"']*)?["']"#));
    static KEYED: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^([A-Za-z][\w.-]*)\s*="));
    static JVM: LazyLock<Regex> = LazyLock::new(|| rx(r#"<artifactId>([\w.-]+)<|['"]([\w.-]+):[\w.-]+:|Include="([\w.-]+)""#));
    let fname = basename(rel);
    if fname.ends_with(".json") {
        let Ok(serde_json::Value::Object(j)) = serde_json::from_str::<serde_json::Value>(text) else { return vec![] };
        return ["dependencies", "peerDependencies", "require"]
            .iter()
            .filter_map(|s| j.get(*s).and_then(|v| v.as_object()))
            .flat_map(|o| o.keys().cloned())
            .collect();
    }
    if fname == "go.mod" {
        return GO.captures_iter(text).map(|c| c[1].to_string()).collect();
    }
    if fname == "Gemfile" {
        return GEM.captures_iter(text).map(|c| c[1].to_string()).collect();
    }
    if fname.ends_with(".txt") {
        return REQ.captures_iter(text).map(|c| c[1].to_string()).collect();
    }
    if fname.ends_with(".toml") || fname.ends_with(".cfg") || fname.ends_with(".py") || fname == "Pipfile" {
        let heads: Vec<(usize, usize, String)> = SECTION.captures_iter(text).map(|c| {
            let m = c.get(0).unwrap();
            (m.start(), m.end(), c[1].to_string())
        }).collect();
        let mut parts: Vec<(String, &str)> = vec![];
        if heads.is_empty() {
            parts.push((String::new(), text));
        }
        for (i, (_, end, head)) in heads.iter().enumerate() {
            let stop = heads.get(i + 1).map(|h| h.0).unwrap_or(text.len());
            parts.push((head.clone(), &text[*end..stop]));
        }
        let mut out = vec![];
        for (head, body) in parts {
            if DEV.is_match(&head) {
                continue; // tools for working on it, not for running it
            }
            out.extend(QUOTED.captures_iter(body).map(|c| c[1].to_string())); // "fastapi[std]>=1"
            if head.ends_with("dependencies") || head.ends_with("packages") {
                out.extend(KEYED.captures_iter(body).map(|c| c[1].to_string())); // poetry / Pipfile style: django = "^4"
            }
        }
        return out;
    }
    // Maven, Gradle, .csproj
    JVM.captures_iter(text).filter_map(|c| c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str().to_string())).collect()
}

/// Every way the project is started: a command in a launch file, or a command one of its own files runs
/// (a container entrypoint picking `gunicorn app.wsgi` or `celery -A app worker`).
pub fn find_launches(project: &Project, files: &[File], find: &Finder, deps: &BTreeSet<String>) -> Vec<Launch> {
    static WORDS: LazyLock<Regex> = LazyLock::new(|| rx(r#"[^\s"'\[\],]+"#));
    static NAMEWORD: LazyLock<Regex> = LazyLock::new(|| rx(r"[A-Za-z][\w-]+"));
    let (mut raw, where_) = read_launch_files(project);
    let wf = |p: &str| where_.get(p);
    let by_path: HashMap<&str, &File> = files.iter().map(|f| (f.path.as_str(), f)).collect();
    // a compose service runs in its image's WORKDIR unless it says otherwise
    let image_cwd: Vec<Option<String>> = raw.iter().map(|L| match &L.image {
        Some(Some(img)) => raw.iter().find(|M| &M.file == img).and_then(|D| D.cwd.clone()),
        _ => None,
    }).collect();
    for (L, c) in raw.iter_mut().zip(image_cwd) {
        if L.cwd.is_none() {
            L.cwd = c;
        }
    }
    for L in raw.iter_mut() {
        if let Some(c) = L.cwd.clone().filter(|c| c.starts_with('/')) {
            L.cwd = where_.get(&c);
        }
        let d = L.cwd.clone().unwrap_or_else(|| dirname(&L.file).to_string());
        L.pages = find.command(&L.serves, &d, false, Some(&wf));
        let mut t = find.command(&L.cmd, &d, L.cwd.is_some(), Some(&wf));
        t.extend(L.pages.iter().cloned());
        L.targets = dedup(t);
    }
    let mains: Vec<&str> = files.iter().filter(|f| f.entry && f.layer != Some("tests")).map(|f| f.path.as_str()).collect();
    // an image that runs something built from the code it copies in: that code's main()
    let program = |L: &Launch| -> Vec<String> {
        let copied = L.copied.clone().unwrap_or_default();
        let inside: Vec<&str> = mains.iter().copied()
            .filter(|m| copied.iter().any(|s| s.is_empty() || m == s || m.starts_with(&format!("{s}/"))))
            .collect();
        let top = inside.iter().map(|m| m.matches('/').count()).min().unwrap_or(0);
        inside.into_iter().filter(|m| m.matches('/').count() == top).map(String::from).collect()
    };
    let words = |c: &str| -> Vec<String> { WORDS.find_iter(c).map(|m| m.as_str().to_string()).collect() };
    let mut built: HashMap<usize, Vec<String>> = HashMap::new(); // launch -> the arguments it passes to a program built here
    for (i, L) in raw.iter_mut().enumerate() {
        // ENTRYPOINT ["/app"], CMD ["air"]: not a file of the repo, nor a tool it depends on, so the image
        // runs a program built from the code it copied in
        let w = words(&L.cmd);
        if L.targets.is_empty() && L.copied.as_ref().is_some_and(|c| !c.is_empty()) && !w.is_empty()
            && !w.iter().take(2).any(|x| deps.contains(&basename(x).to_lowercase()))
        {
            L.targets = program(L);
            built.insert(i, w[1..].to_vec());
        }
    }
    for i in 0..raw.len() {
        if raw[i].targets.is_empty() {
            if let Some(Some(img)) = raw[i].image.clone() {
                // a compose service of an image built here: `command: ["serve"]` are its arguments
                if let Some(D) = raw.iter().position(|M| M.file == img) {
                    if built.contains_key(&D) {
                        raw[i].targets = raw[D].targets.clone();
                        built.insert(i, words(&raw[i].cmd));
                    }
                }
            }
        }
        if let (Some(args), Some(first)) = (built.get(&i), raw[i].targets.first().cloned()) {
            // `serve`, `worker`: the one file of the program with that name
            let home = dirname(&first).to_string();
            let lang = by_path.get(first.as_str()).map(|f| f.lang.name);
            for w in args.iter().flat_map(|a| NAMEWORD.find_iter(a).map(|m| m.as_str().to_string()).collect::<Vec<_>>()) {
                let hits: Vec<&str> = files.iter()
                    .filter(|q| q.path.starts_with(&home) && basename(&q.path).rsplit_once('.').map(|x| x.0) == Some(w.as_str()) && Some(q.lang.name) == lang)
                    .map(|q| q.path.as_str())
                    .collect();
                if hits.len() == 1 && !raw[i].targets.iter().any(|t| t == hits[0]) {
                    raw[i].targets.push(hits[0].to_string());
                }
            }
        }
    }
    let mut found = raw;
    found.extend(spawned(files, find, &where_));
    let n = found.len();
    for i in 0..n {
        // a command started by another one: by an image it was baked into, or from a file it runs
        let via = found[i].parent.or_else(|| (0..n).find(|&j| j != i && found[j].targets.contains(&found[i].file)));
        found[i].via = via;
    }
    // Docker / compose / Procfile before Makefile; keep `via` pointing at the same launch after sorting
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| if found[i].via.is_none() { found[i].rank } else { -1 });
    let pos: HashMap<usize, usize> = order.iter().enumerate().map(|(n, &o)| (o, n)).collect();
    order
        .iter()
        .map(|&i| {
            let mut L = found[i].clone();
            L.via = L.via.map(|v| pos[&v]);
            L.parent = None;
            L
        })
        .collect()
}

/// Commands a program builds and runs itself: the strings handed to a call that starts another program
/// (subprocess.run, exec.Command, os.execvp ...), directly or through a variable it names.
fn spawned(files: &[File], find: &Finder, where_: &Where) -> Vec<Launch> {
    static IDENT: LazyLock<Regex> = LazyLock::new(|| rx(r"[A-Za-z_]\w*"));
    static KEY: LazyLock<Regex> = LazyLock::new(|| rx(r#"["']([\w-]+)["']\s*:"#));
    let proc = &IO_RES.iter().find(|(k, _)| *k == "proc").unwrap().1;
    let wf = |p: &str| where_.get(p);
    let mut out = vec![];
    for f in files {
        if f.layer == Some("tests") || !proc.is_match(&f.code) {
            continue;
        }
        let cwd = CHDIR.captures(&f.bare).and_then(|cd| {
            let c = &cd[1];
            if c.starts_with('/') { where_.get(c) } else { Some(repo_path(dirname(&f.path), c)) }
        });
        let d = cwd.clone().unwrap_or_else(|| dirname(&f.path).to_string());
        // the regions of the spawning calls
        let mut regions: Vec<(usize, usize)> = vec![];
        for m in proc.find_iter(&f.code) {
            let call = f.calls.iter().filter(|c| !c.is_ref && c.at >= m.start() && c.at <= m.end() + 2).min_by_key(|c| c.at);
            match call {
                Some(c) => regions.push((c.at, c.end.max(m.end()))),
                None => regions.push((m.start(), f.code[m.end()..].find('\n').map(|n| m.end() + n).unwrap_or(f.code.len()))),
            }
        }
        // strings inside them, and in the values of the names they use (CMDS[sys.argv[1]])
        let mut spans: Vec<(usize, usize)> = regions.clone();
        for &(a, b) in &regions {
            for id in IDENT.find_iter(crate::text::slice(&f.code, a, b)) {
                let name = id.as_str();
                if name.len() < 2 {
                    continue;
                }
                let assign = Regex::new(&format!(r"(?m)^[ \t]*(?:(?:const|let|var|local|readonly|export)\s+)?{}\s*(?::[^=\n]+)?(?::=|=)\s*", regex::escape(name))).unwrap();
                for m in assign.find_iter(&f.bare) {
                    spans.push((m.end(), value_end(&f.bare, m.end())));
                }
            }
        }
        spans.sort();
        spans.dedup();
        let mut seen = BTreeSet::new();
        for (a, b) in spans {
            let piece = crate::text::slice(&f.bare, a, b);
            let mut cands: Vec<(String, usize)> = STRING
                .captures_iter(piece)
                .filter_map(|c| c.get(1).or(c.get(2)).or(c.get(3)).map(|m| (m.as_str().to_string(), a + m.start())))
                .filter(|(s, _)| s.split_whitespace().count() >= 2)
                .collect();
            if f.lang.name == "Shell" {
                cands.push((piece.lines().next().unwrap_or("").to_string(), a)); // `exec gunicorn app.wsgi`
            }
            for (s, at) in cands {
                if !seen.insert((s.clone(), at)) {
                    continue;
                }
                let hits: Vec<String> = find.command(&s, &d, cwd.is_some(), Some(&wf)).into_iter().filter(|h| *h != f.path).collect();
                if hits.is_empty() {
                    continue;
                }
                let mut from = at.saturating_sub(300);
                while !f.bare.is_char_boundary(from) {
                    from += 1;
                }
                let before = &f.bare[from..at];
                let key = KEY.captures_iter(before).last().map(|c| c[1].to_string());
                let name = key.unwrap_or_else(|| s.split_whitespace().find(|w| !WRAPPERS.contains(w)).unwrap_or("").to_string());
                out.push(Launch { name, cmd: s, file: f.path.clone(), cwd: cwd.clone(), targets: dedup(hits), rank: -1, ..Default::default() });
            }
        }
    }
    out
}

/// Where the value assigned at `at` ends: its closing bracket, else the end of the line.
fn value_end(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    let Some(first) = rest.chars().next() else { return at };
    let close = match first {
        '{' => '}',
        '[' => ']',
        '(' => ')',
        _ => return at + rest.find('\n').unwrap_or(rest.len()),
    };
    let mut depth = 0;
    for (i, c) in rest.char_indices() {
        if c == first {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return at + i + 1;
            }
        }
        if i > 20_000 {
            break;
        }
    }
    at + rest.len().min(20_000)
}
