//! The analysis pipeline: scan -> parse -> resolve imports + link paths and names -> resolve calls -> classify
//! layers -> group into parts -> build tree + dependency edges -> find programs and their journeys -> one JSON
//! model the viewer draws.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use rayon::prelude::*;
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::finder::Finder;
use crate::graph::{self, Gid};
use crate::journey::{step_io, steps_json, Ctx, Step, Walk, MAX_ROUTES, MAX_ROUTE_STEPS, MAX_STEPS};
use crate::lang::LANGS;
use crate::launch::{dependencies, find_launches, Launch};
use crate::layers::{build_units, classify, conventions, stem, Tally};
use crate::parse::{self, File, Kind};
use crate::resolve::{basename, dirname, Project};
use crate::rules::VERB_METHODS;
use crate::tables::{cat_of, short as layer_short, toks, ENTRY_STEMS, EXT_CATS, IO, LAYERS, TOKEN_LAYER, WRAPPERS};
use crate::text::read_para;

/// A program the project runs.
#[derive(Clone, Debug)]
struct Proc {
    name: String,
    cmd: String,
    /// launches, outermost first
    chain: Vec<Launch>,
    starts: Vec<String>,
    key: Vec<String>,
    /// other files that start the same program (python -m pkg next to a console script)
    also: Vec<String>,
}

fn words(c: &str) -> Vec<&str> {
    static W: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[^\s"'\[\],]+"#).unwrap());
    W.find_iter(c).map(|m| m.as_str()).collect()
}

/// 'gunicorn app.wsgi --bind 0.0.0.0' -> 'gunicorn app.wsgi'; 'celery -A app worker -l INFO' -> 'celery -A app worker'
fn short(cmd: &str) -> String {
    let mut out: Vec<&str> = vec![];
    let ws = words(cmd);
    let skip = ws.iter().take_while(|w| WRAPPERS.contains(w)).count();
    for w in ws.into_iter().skip(skip) {
        if w.starts_with("--") || out.len() == 4 || (w.starts_with('-') && out.len() >= 3) {
            break;
        }
        if !w.starts_with('$') {
            out.push(w);
        }
    }
    out.join(" ")
}

fn both(xs: &[String]) -> String {
    match xs.len() {
        0 => String::new(),
        1 => xs[0].clone(),
        n => format!("{} and {}", xs[..n - 1].join(", "), xs[n - 1]),
    }
}

fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    // days since 1970-01-01 -> civil date (Howard Hinnant's algorithm)
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Map a repository; `index.scip` in it (or the file given) sharpens which definition each call runs.
pub fn analyze(root: &Path, name: &str) -> Result<Value, String> {
    analyze_with(root, name, None)
}

pub fn analyze_with(root: &Path, name: &str, scip: Option<&Path>) -> Result<Value, String> {
    let (mut files, man) = crate::scan::scan(root);
    if files.is_empty() {
        return Err(format!("no source files found in {}", root.display()));
    }
    files.par_iter_mut().for_each(parse::parse); // symbols, calls, imports, doc comment
    let project = Project::new(root, &files, man.clone());
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let find = Finder::new(&paths);
    let refs = graph::link(&files, &find); // paths and names, any language
    for (i, f) in files.iter_mut().enumerate() {
        let (uses, ext) = project.resolve(f);
        f.bound = project.bind(f);
        f.bound_names = project.bind_names(f);
        let mut u: BTreeSet<String> = uses;
        u.extend(refs[i].iter().cloned());
        u.remove(&f.path);
        f.uses = u.into_iter().collect();
        f.ext = ext.into_iter().collect();
    }
    // layers first: repo code never calls into a test file's helpers
    let top_dirs: HashSet<String> = files.iter().filter(|f| f.path.contains('/')).map(|f| f.path.split('/').next().unwrap().to_lowercase()).collect();
    let fullstack = ["client", "frontend", "web"].iter().any(|d| top_dirs.contains(*d)) && ["server", "backend", "api"].iter().any(|d| top_dirs.contains(*d));
    let orm = crate::orm::index(&files);
    for f in files.iter_mut() {
        f.layer = classify(f, fullstack);
        graph::def_io(f, &[], &orm);
    }
    let known: HashSet<String> = paths.iter().cloned().collect();
    let scip_index = match scip {
        Some(p) => Some(crate::scip::Scip::load(p, &known)?),
        None => Some(root.join("index.scip")).filter(|p| p.is_file()).and_then(|p| crate::scip::Scip::load(&p, &known).ok()),
    };
    let (targets, guess, scip_hits) = graph::resolve_calls(&mut files, &project.rust_libs(), scip_index.as_ref());
    let mounts = crate::mounts::build(&files);
    for (i, f) in files.iter_mut().enumerate() {
        // a call into another file is a link to it, whether or not it was imported by name
        let mut u: BTreeSet<String> = f.uses.iter().cloned().collect();
        for t in targets[i].iter().flatten() {
            u.insert(paths[t.0].clone());
        }
        u.remove(&f.path);
        f.uses = u.into_iter().collect();
    }
    let ix: HashMap<String, usize> = paths.iter().enumerate().map(|(i, p)| (p.clone(), i)).collect();
    // `prisma.user.findMany()`: a client the repo's own module opens carries that module's database to the caller
    let carried: Vec<Vec<(String, &'static str)>> = files.iter().map(|f| {
        f.bound.iter().flat_map(|(name, p)| {
            files[ix[p]].defs[0].io.iter().filter(|k| matches!(**k, "db" | "queue" | "http_out")).map(move |k| (name.clone(), *k))
        }).collect()
    }).collect();
    for (f, c) in files.iter_mut().zip(carried) {
        if !c.is_empty() {
            graph::def_io(f, &c, &orm);
        }
        if f.layer == Some("tests") {
            f.io.clear();
        }
    }

    // ---- files the framework loads by convention, and the commands that start the program
    let (conv_found, mut conv_loose) = conventions(&files);
    // a routes file no program reaches is read by the framework when the program starts
    let unreached: Vec<String> = files.iter()
        .filter(|f| crate::rules::routes_file(&f.path).is_some() && f.layer != Some("tests"))
        .map(|f| f.path.clone())
        .collect();
    conv_loose.extend(unreached);
    let mut conv: BTreeSet<(String, String)> = BTreeSet::new();
    for (a, ms) in &conv_found {
        for m in ms {
            if !files[ix[a]].uses.contains(m) {
                conv.insert((a.clone(), m.clone()));
            }
        }
    }
    for (a, m) in &conv {
        files[ix[a]].uses.push(m.clone());
    }
    let used_anywhere: HashSet<&String> = files.iter().flat_map(|f| f.uses.iter()).collect();
    let conv_roots: BTreeSet<String> = conv_loose.iter().cloned().chain(conv_found.keys().filter(|a| !used_anywhere.contains(a)).cloned()).collect();
    let deps: BTreeSet<String> = man.get("DEPS").into_iter().flatten().flat_map(|rel| dependencies(rel, &project.read(rel))).map(|w| w.to_lowercase()).collect();
    let launches = find_launches(&project, &files, &find, &deps);

    // ---- parts (units) and layer assignment
    let U = build_units(&files, false);
    let units = &U.units;
    let layer_name: HashMap<&str, &str> = LAYERS.iter().map(|l| (l.0, l.1)).collect();
    let mut unit_of: HashMap<String, String> = HashMap::new();
    let mut ulayer: BTreeMap<String, &'static str> = BTreeMap::new();
    for u in units {
        let mut c: Tally<&'static str> = Tally::default();
        for p in &u.files {
            let f = &files[ix[p]];
            c.add(f.layer.unwrap_or("logic"), f.loc); // files with no clue count as logic, so one small named file can't decide a folder
        }
        let ly = u.layer.unwrap_or_else(|| c.top().unwrap_or("logic"));
        ulayer.insert(u.id.clone(), ly);
        for p in &u.files {
            let f = &mut files[ix[p]];
            f.layer = f.layer.or(Some(ly));
            unit_of.insert(p.clone(), u.id.clone());
        }
    }
    let lay = |files: &[File], p: &str| files[ix[p]].layer.unwrap_or("logic");

    // ---- processes: launch commands > declared / main() entry points > conventional file names > public API
    let reach = |files: &[File], starts: &[String]| -> BTreeSet<String> {
        let mut seen: BTreeSet<String> = starts.iter().cloned().collect();
        let mut todo: Vec<String> = starts.to_vec();
        while let Some(p) = todo.pop() {
            for q in &files[ix[&p]].uses {
                if !seen.contains(q) && lay(files, q) != "tests" {
                    seen.insert(q.clone());
                    todo.push(q.clone());
                }
            }
        }
        seen
    };
    let mut procs: Vec<Proc> = vec![];
    let mut entry_kind = "Entry points";
    for (li, L) in launches.iter().enumerate() {
        if launches.iter().any(|M| M.via == Some(li)) {
            continue; // it starts another command: part of the way in, not a process of its own
        }
        let mut chain = vec![li];
        let mut P = L.via;
        while let Some(p) = P.filter(|p| !chain.contains(p) && chain.len() < 6) {
            chain.insert(0, p);
            P = launches[p].via;
        }
        let mut said = toks(&L.name);
        for w in words(&L.cmd) {
            if !w.contains(['/', '.', '$', '-']) {
                said.extend(toks(w));
            }
        }
        if chain.len() == 1 && said.iter().any(|t| matches!(TOKEN_LAYER.get(t.as_str()), Some(&"tests" | &"config"))) {
            continue; // test, lint, build, install, deploy ...
        }
        let mut starts: Vec<String> = L.targets.iter().filter(|s| lay(&files, s) != "tests").cloned().collect();
        static TOOLW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[^\s"'\[\],=]+"#).unwrap());
        let tool: Vec<String> = TOOLW.find_iter(&L.cmd).take(2).map(|m| m.as_str()).filter(|w| !w.starts_with('-')).map(|w| basename(w).to_lowercase()).collect();
        if starts.is_empty() && tool.iter().any(|t| deps.contains(t)) {
            // a tool the project depends on runs it from a folder (`next dev`): it finds main.py / index.js there,
            // or convention files
            let d = L.cwd.clone().unwrap_or_else(|| dirname(&L.file).to_string());
            let inside: Vec<&String> = paths.iter().filter(|q| (d.is_empty() || q.starts_with(&format!("{d}/"))) && lay(&files, q) != "tests").collect();
            let users: HashSet<(&str, &String)> = files.iter().filter(|f| f.layer != Some("tests")).flat_map(|f| f.uses.iter().map(move |u| (f.lang.name, u))).collect();
            let named: Vec<&String> = inside.iter().copied()
                .filter(|q| {
                    let f = &files[ix[*q]];
                    ENTRY_STEMS.contains(&stem(q)) && f.lang.entry_by_name && !users.contains(&(f.lang.name, *q))
                        && q[d.len()..].trim_matches('/').matches('/').count() <= 2
                })
                .collect();
            let top = named.iter().map(|q| q.matches('/').count()).min().unwrap_or(0);
            starts = named.into_iter().filter(|q| q.matches('/').count() == top).cloned().collect();
            let near: HashSet<&str> = starts.iter().map(|q| dirname(q)).collect();
            let extra: Vec<String> = conv_roots.iter().filter(|m| inside.contains(m) && (starts.is_empty() || near.contains(dirname(m)))).cloned().collect();
            starts.extend(extra);
        }
        let key: Vec<String> = starts.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect();
        let chain: Vec<Launch> = chain.iter().map(|&i| launches[i].clone()).collect();
        if let Some(same) = procs.iter_mut().find(|P| P.key == key) {
            // one program started several ways: keep the names given in the same kind of file
            let names_: Vec<String> = same.name.split(" / ").map(String::from).collect();
            if !names_.contains(&L.name) && names_.len() < 3 && chain[0].rank == same.chain[0].rank {
                same.name = format!("{} / {}", same.name, L.name);
            }
        } else if !starts.is_empty() {
            let mut seen = BTreeSet::new();
            starts.retain(|s| seen.insert(s.clone()));
            procs.push(Proc { name: L.name.clone(), cmd: L.cmd.clone(), chain, starts, key, also: vec![] });
        }
    }
    let declared: HashSet<&String> = project.declared.iter().filter(|h| ix.contains_key(*h)).collect();
    let imported: HashSet<&String> = files.iter().filter(|f| !matches!(f.layer, Some("tests" | "config"))).flat_map(|f| f.uses.iter()).collect();
    let mut cand: Vec<((usize, usize, i64), String)> = vec![];
    for f in &files {
        if f.layer == Some("tests") {
            continue;
        }
        let pr = if declared.contains(&f.path) {
            0
        } else if f.entry {
            1
        } else if ENTRY_STEMS.contains(&stem(&f.path)) && f.path.matches('/').count() <= 2 && f.lang.entry_by_name && !imported.contains(&f.path) {
            2
        } else {
            continue;
        };
        cand.push(((pr, f.path.matches('/').count(), -(f.loc as i64)), f.path.clone()));
    }
    cand.sort();
    let not_config: Vec<_> = cand.iter().filter(|(_, p)| lay(&files, p) != "config").cloned().collect();
    let pool = if not_config.is_empty() { cand } else { not_config };
    let mut reached: BTreeSet<String> = BTreeSet::new();
    for P in &procs {
        reached.extend(reach(&files, &P.starts));
        for c in &P.chain {
            reached.insert(c.file.clone());
            reached.extend(c.targets.iter().cloned());
        }
    }
    let launched = !procs.is_empty();
    for ((pr, ..), p) in &pool {
        // programs no launch command covers: one process each
        if !reached.contains(p) && !(launched && *pr == 2) {
            procs.push(Proc { name: p.clone(), cmd: String::new(), chain: vec![], starts: vec![p.clone()], key: vec![p.clone()], also: vec![] });
            reached.extend(reach(&files, std::slice::from_ref(p)));
        }
    }
    for P in &procs {
        // a start that names nothing in the project boots it by scanning the folder below it
        let d = dirname(&P.starts[0]).to_string();
        if !d.is_empty() && !P.starts.iter().any(|s| !files[ix[s]].uses.is_empty()) {
            let users: HashSet<String> = files.iter().flat_map(|f| f.uses.iter().cloned()).collect();
            let add: Vec<String> = paths.iter()
                .filter(|q| q.starts_with(&format!("{d}/")) && !users.contains(*q) && lay(&files, q) != "tests" && !P.starts.contains(q))
                .cloned()
                .collect();
            for q in add {
                files[ix[&P.starts[0]]].uses.push(q.clone());
                conv.insert((P.starts[0].clone(), q));
            }
        }
    }
    let lone = |files: &[File], P: &Proc| {
        P.cmd.is_empty() && reach(files, &P.starts).len() == P.starts.iter().collect::<BTreeSet<_>>().len()
            && P.starts.iter().all(|s| files[ix[s]].text.starts_with("#!"))
    };
    if procs.iter().any(|P| !lone(&files, P)) {
        procs.retain(|P| !lone(&files, P)); // bin/bundle, mvnw: scripts wrapping a tool, not the program itself
    }
    let dep_dirs: Vec<&str> = man.get("DEPS").into_iter().flatten().map(|m| dirname(m)).collect();
    let home = |q: &str| -> String {
        let mut best = "";
        for d in &dep_dirs {
            if (q.starts_with(&format!("{d}/")) || d.is_empty()) && d.len() > best.len() {
                best = d;
            }
        }
        best.to_string()
    };
    let mut on_way: BTreeSet<String> = BTreeSet::new();
    for P in &procs {
        on_way.extend(reach(&files, &P.starts));
    }
    for m in conv_loose.iter().filter(|m| !on_way.contains(*m)) {
        // loaded by the framework of the project it sits in, for each of its programs
        for P in &procs {
            // a routes file is read by a program written in its own language, not by any program next to it
            let same_language = crate::rules::routes_file(m).is_none() || graph::family(files[ix[&P.starts[0]]].lang) == graph::family(files[ix[m]].lang);
            if same_language && home(&P.starts[0]) == home(m) {
                files[ix[&P.starts[0]]].uses.push(m.clone());
                conv.insert((P.starts[0].clone(), m.clone()));
            }
        }
    }
    if procs.is_empty() {
        // a library: its "front door" is the package's top-level index
        let lib: Vec<&String> = paths.iter().filter(|p| ["__init__", "index", "lib", "mod"].contains(&stem(p)) && lay(&files, p) != "tests" && p.matches('/').count() <= 3).collect();
        if !lib.is_empty() {
            let m = lib.iter().map(|p| p.matches('/').count()).min().unwrap();
            let mut starts: Vec<String> = lib.into_iter().filter(|p| p.matches('/').count() == m).cloned().collect();
            starts.sort_by_key(|p| std::cmp::Reverse(files[ix[p]].uses.len()));
            starts.truncate(3);
            procs = vec![Proc { name: "Public API".into(), cmd: String::new(), chain: vec![], key: starts.clone(), starts, also: vec![] }];
            entry_kind = "Public API";
        }
    }
    let mut merged: Vec<Proc> = vec![];
    for P in procs {
        // bin/rails and bin/rake lead to the same program
        let rp = reach(&files, &P.starts);
        let same = merged.iter_mut().find(|M| {
            reach(&files, &M.starts) == rp && M.cmd.is_empty() && P.cmd.is_empty() && dirname(&M.starts[0]) == dirname(&P.starts[0])
        });
        if let Some(same) = same {
            if same.name.matches(" / ").count() < 2 {
                let first = if same.name.contains(" / ") { same.name.clone() } else { basename(&same.name).to_string() };
                same.name = format!("{first} / {}", basename(&P.name));
            }
            for s in P.starts {
                if !same.starts.contains(&s) {
                    same.starts.push(s);
                }
            }
            continue;
        }
        // `python -m pkg` next to the console script it wraps: the wrapper only adds itself
        if P.cmd.is_empty() && P.chain.is_empty() {
            if let Some(M) = merged.iter_mut().find(|M| {
                let rm = reach(&files, &M.starts);
                M.starts.iter().any(|s| dirname(s) == dirname(&P.starts[0])) &&
                M.starts.iter().all(|s| rp.contains(s)) && rp.iter().all(|q| rm.contains(q) || P.starts.contains(q))
            }) {
                M.also.extend(P.starts);
                continue;
            }
        }
        merged.push(P);
    }
    merged.truncate(8);
    let mut procs = merged;
    let mut names_: Tally<String> = Tally::default();
    for P in &procs {
        names_.add(P.name.clone(), 1);
    }
    for P in procs.iter_mut() {
        // two `backend` services: say which file each one comes from
        if names_.get(&P.name) > 1 && !P.chain.is_empty() {
            P.name = format!("{} ({})", P.name, basename(&P.chain[0].file));
        }
    }
    let mut base: Tally<String> = Tally::default();
    for P in &procs {
        base.add(basename(&P.name).to_string(), 1);
    }
    for P in procs.iter_mut() {
        // lesson1/main.go, lesson2/main.go ... keep their folders; a lone app/main.py is just main.py
        if P.cmd.is_empty() && !P.name.contains(" / ") && base.get(&basename(&P.name).to_string()) == 1 {
            P.name = basename(&P.name).to_string();
        }
    }
    let mut entries: Vec<String> = vec![];
    for P in &procs {
        for s in &P.starts {
            if !entries.contains(s) {
                entries.push(s.clone());
            }
        }
    }
    let mut entry_set: HashSet<String> = entries.iter().cloned().collect();
    for P in &procs {
        entry_set.extend(P.chain.iter().map(|c| c.file.clone()).filter(|f| ix.contains_key(f)));
        entry_set.extend(P.also.iter().cloned());
    }

    // ---- dependency edges between files
    let mut fedges: Tally<(String, String)> = Tally::default();
    for f in &files {
        for q in &f.uses {
            if ix.contains_key(q) {
                fedges.add((f.path.clone(), q.clone()), 1);
            }
        }
    }

    // ---- node tree: layer > part > folder > file
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut order: Vec<String> = vec![];
    let mk = |nodes: &mut BTreeMap<String, Node>, order: &mut Vec<String>, id: &str, name: &str, kind: &'static str, path: &str, fl: Vec<String>, parent: Option<String>| {
        let mut langs: Tally<&str> = Tally::default();
        let mut io: BTreeSet<usize> = BTreeSet::new();
        let mut loc = 0;
        for p in &fl {
            let f = &files[ix[p]];
            langs.add(f.lang.name, f.loc);
            loc += f.loc;
            io.extend(f.io.iter().map(|k| crate::tables::io_order(k)));
        }
        let n = Node {
            id: id.into(), name: name.into(), kind, path: path.into(), loc, langs: langs.most_common().into_iter().take(4).map(|(l, c)| (l.to_string(), c)).collect(),
            children: vec![], edges: vec![], uses: vec![], used_by: vec![], parent, layer: None, desc: String::new(),
            io: io.into_iter().map(|i| IO[i].key).collect(), files: fl, unit: false, file: None,
        };
        order.push(id.to_string());
        nodes.insert(id.to_string(), n);
    };
    let file_node = |nodes: &mut BTreeMap<String, Node>, order: &mut Vec<String>, p: &str, parent: &str| {
        mk(nodes, order, p, basename(p), "file", p, vec![p.to_string()], Some(parent.to_string()));
        let f = &files[ix[p]];
        let n = nodes.get_mut(p).unwrap();
        n.layer = f.layer;
        let ext: BTreeSet<&String> = f.ext.iter().map(|(n, _)| n).collect();
        n.file = Some(FileInfo { lang: f.lang.name, entry: entry_set.contains(p), ext: ext.into_iter().take(12).cloned().collect() });
    };
    fn dir_node(U: &crate::layers::Units, files_mk: &mut dyn FnMut(&mut BTreeMap<String, Node>, &mut Vec<String>, NodeReq),
                nodes: &mut BTreeMap<String, Node>, order: &mut Vec<String>, d: &str, parent: &str, label: &str) {
        files_mk(nodes, order, NodeReq::Dir(d.to_string(), label.to_string(), parent.to_string()));
        let mut kids = vec![];
        for s in U.tree.get(d).into_iter().flatten() {
            let s2 = U.squash(s);
            let lab = if d.is_empty() { s2.clone() } else { s2[d.len() + 1..].to_string() };
            dir_node(U, files_mk, nodes, order, &s2, d, &lab);
            kids.push(s2);
        }
        for p in U.loose.get(d).into_iter().flatten() {
            files_mk(nodes, order, NodeReq::File(p.clone(), d.to_string()));
            kids.push(p.clone());
        }
        nodes.get_mut(d).unwrap().children = kids;
    }
    let generic: HashSet<&str> = ["src", "lib", "app", "main", "core", "java", "python", "source", "pkg", "internal", "cmd"].into();
    let unit_name = |u: &crate::layers::Unit| -> String {
        if u.kind == "file" {
            return basename(&u.id).to_string();
        }
        let d = &u.path;
        let mut b = if basename(d).is_empty() { name.to_string() } else { basename(d).to_string() };
        if u.kind == "dir" {
            if generic.contains(b.as_str()) {
                let parts: Vec<&str> = d.split('/').collect();
                if let Some(a) = parts[..parts.len() - 1].iter().rev().find(|a| !generic.contains(**a)) {
                    b = format!("{a}/{b}");
                }
            }
            return b;
        }
        let label = if d.is_empty() { "project root".to_string() } else { b };
        if u.multi { format!("{label} ({})", layer_name[u.layer.unwrap()]) } else { format!("{label} files") }
    };
    let mut layer_units: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    {
        let mut make = |nodes: &mut BTreeMap<String, Node>, order: &mut Vec<String>, req: NodeReq| match req {
            NodeReq::File(p, parent) => file_node(nodes, order, &p, &parent),
            NodeReq::Dir(d, label, parent) => mk(nodes, order, &d, &label, "dir", &d, U.sub[&d].clone(), Some(parent)),
        };
        for u in units {
            let lk = ulayer[&u.id];
            let lid = format!("layer:{lk}");
            match u.kind {
                "file" => make(&mut nodes, &mut order, NodeReq::File(u.id.clone(), lid)),
                "dir" => dir_node(&U, &mut make, &mut nodes, &mut order, &u.id, &lid, &unit_name(u)),
                _ => {
                    mk(&mut nodes, &mut order, &u.id, &unit_name(u), "dir", &u.path, u.files.clone(), Some(lid));
                    let mut kids: Vec<String> = u.files.clone();
                    kids.sort();
                    for p in &kids {
                        make(&mut nodes, &mut order, NodeReq::File(p.clone(), u.id.clone()));
                    }
                    nodes.get_mut(&u.id).unwrap().children = kids;
                }
            }
            let n = nodes.get_mut(&u.id).unwrap();
            n.unit = true;
            n.layer = Some(lk);
            layer_units.entry(lk).or_default().push(u.id.clone());
        }
    }
    for n in nodes.values_mut() {
        // folders inside a part take the layer that dominates them
        if n.kind == "dir" && !n.unit {
            let mut c: Tally<&'static str> = Tally::default();
            for p in &n.files {
                c.add(lay(&files, p), files[ix[p]].loc);
            }
            n.layer = c.top();
        }
    }

    // ---- dependency edges between siblings (drawn as lines in the GUI; a 4th item 1 = only convention links)
    let edge = |a: &str, b: &str, w: usize, plain: &Tally<(String, String)>| -> Value {
        if plain.get(&(a.to_string(), b.to_string())) > 0 { json!([a, b, w]) } else { json!([a, b, w, 1]) }
    };
    let ids: Vec<String> = order.clone();
    for id in &ids {
        let n = &nodes[id];
        if n.children.is_empty() {
            continue;
        }
        let mut owner: HashMap<&str, &str> = HashMap::new();
        for c in &n.children {
            for p in &nodes[c].files {
                owner.insert(p, c);
            }
        }
        let mut cnt: Tally<(String, String)> = Tally::default();
        let mut plain: Tally<(String, String)> = Tally::default();
        for p in &n.files {
            for q in &files[ix[p]].uses {
                if let (Some(a), Some(b)) = (owner.get(p.as_str()), owner.get(q.as_str())) {
                    if a != b {
                        let k = (a.to_string(), b.to_string());
                        cnt.add(k.clone(), 1);
                        plain.add(k, usize::from(!conv.contains(&(p.clone(), q.clone()))));
                    }
                }
            }
        }
        let es: Vec<Value> = cnt.most_common().into_iter().take(80).map(|((a, b), w)| edge(&a, &b, w, &plain)).collect();
        nodes.get_mut(id).unwrap().edges = es;
    }
    let mut uedges: Tally<(String, String)> = Tally::default();
    let mut uplain: Tally<(String, String)> = Tally::default();
    for ((p, q), _) in fedges.items() {
        if unit_of[&p] != unit_of[&q] {
            let k = (unit_of[&p].clone(), unit_of[&q].clone());
            uedges.add(k.clone(), 1);
            uplain.add(k, usize::from(!conv.contains(&(p.clone(), q.clone()))));
        }
    }
    let mut uses_w: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    let mut used_w: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for ((p, q), w) in fedges.items() {
        uses_w.entry(p.clone()).or_default().push((q.clone(), w));
        used_w.entry(q).or_default().push((p, w));
    }
    for ((a, b), w) in uedges.items() {
        if nodes[&a].kind != "file" {
            uses_w.entry(a.clone()).or_default().push((b.clone(), w));
        }
        if nodes[&b].kind != "file" {
            used_w.entry(b).or_default().push((a, w));
        }
    }
    for (id, n) in nodes.iter_mut() {
        let mut u = uses_w.remove(id).unwrap_or_default();
        u.sort_by_key(|a| std::cmp::Reverse(a.1));
        n.uses = u.into_iter().map(|x| x.0).collect();
        let mut u = used_w.remove(id).unwrap_or_default();
        u.sort_by_key(|a| std::cmp::Reverse(a.1));
        n.used_by = u.into_iter().map(|x| x.0).collect();
    }

    // ---- layers
    let mut layers_out = vec![];
    let mut luse: HashMap<&str, Tally<&str>> = HashMap::new();
    for ((a, b), w) in uedges.items() {
        if ulayer[&a] != ulayer[&b] {
            luse.entry(ulayer[&a]).or_default().add(ulayer[&b], w);
        }
    }
    for (key, nm, tag, color, grp) in LAYERS {
        let Some(us) = layer_units.get_mut(key) else { continue };
        us.sort_by_key(|i| std::cmp::Reverse(nodes[i].loc));
        let us = us.clone();
        let fl: Vec<String> = us.iter().flat_map(|i| nodes[i].files.clone()).collect();
        let lid = format!("layer:{key}");
        mk(&mut nodes, &mut order, &lid, nm, "layer", "", fl.clone(), None);
        let inside: HashSet<&String> = us.iter().collect();
        let es: Vec<Value> = uedges.most_common().into_iter().filter(|((a, b), _)| inside.contains(a) && inside.contains(b)).take(80).map(|((a, b), w)| edge(&a, &b, w, &uplain)).collect();
        let n = nodes.get_mut(&lid).unwrap();
        n.layer = Some(key);
        n.children = us.clone();
        n.desc = tag.to_string();
        n.edges = es;
        let loc = n.loc;
        let lu: Vec<&str> = luse.get(key).map(|t| t.most_common().into_iter().take(3).map(|x| x.0).collect()).unwrap_or_default();
        layers_out.push(json!({"key": key, "name": nm, "tag": tag, "color": color, "group": grp, "units": us, "files": fl.len(), "loc": loc, "uses": lu}));
    }

    // ---- plain-English descriptions
    let mut readme_dir: HashMap<&str, &String> = HashMap::new();
    for r in man.get("README").into_iter().flatten().rev() {
        readme_dir.insert(dirname(r), r);
    }
    let describe = |n: &Node| -> String {
        if n.kind == "layer" {
            return n.desc.clone();
        }
        if n.kind == "file" {
            let f = &files[ix[&n.id]];
            if !f.doc.is_empty() {
                return f.doc.clone();
            }
            let names: Vec<&str> = f.symbols.iter().filter(|s| ["class", "function", "component", "type"].contains(&s.k) && !s.n.starts_with('_')).map(|s| s.n.as_str()).collect();
            if f.layer == Some("tests") && names.is_empty() {
                return "Test code.".into();
            }
            if !names.is_empty() {
                return format!("Defines {}{}", names[..names.len().min(3)].join(", "), if names.len() > 3 { "…" } else { "." });
            }
            let routes = f.symbols.iter().filter(|s| s.k == "route").count();
            if routes > 0 {
                return format!("Handles {}.", plural(routes, "web endpoint"));
            }
            return format!("{} file, {} lines.", f.lang.name, f.loc);
        }
        if n.kind == "dir" {
            if let Some(r) = readme_dir.get(n.id.as_str()) {
                let t = read_para(&project.read(r));
                if !t.is_empty() {
                    return t;
                }
            }
            for st in ["__init__", "index", "mod", "lib", "doc", "main"] {
                for p in U.loose.get(&n.id).into_iter().flatten() {
                    if stem(p) == st && !files[ix[p]].doc.is_empty() {
                        return files[ix[p]].doc.clone();
                    }
                }
            }
        }
        let mut by_size: Vec<&String> = n.files.iter().collect();
        by_size.sort_by_key(|p| std::cmp::Reverse(files[ix[*p]].loc));
        let top: Vec<&str> = by_size.into_iter().map(|p| stem(p)).filter(|s| !["__init__", "index", "mod"].contains(s)).take(3).collect();
        let s = format!("{}. {}", n.layer.map(layer_short).unwrap_or(""), plural(n.files.len(), "file"));
        if top.is_empty() { format!("{s}.") } else { format!("{s}, including {}.", top.join(", ")) }
    };
    let descs: Vec<(String, String)> = nodes.values().map(|n| (n.id.clone(), describe(n))).collect();
    for (id, d) in descs {
        nodes.get_mut(&id).unwrap().desc = d;
    }

    // ---- journeys: for each process, every step from where it starts to where it ends
    let ctx = Ctx { files: &files, targets: &targets, guess: &guess, ix: &ix };
    let io_label: HashMap<&str, &str> = IO.iter().map(|i| (i.key, i.label)).collect();
    let mut flow = Value::Null;
    if !procs.is_empty() {
        let mut out = vec![];
        let mut seen_units: HashSet<String> = HashSet::new();
        for P in &procs {
            let mut walk = Walk::new(&ctx, MAX_STEPS);
            // the command names the function it runs (`pkg.cli:main`), else the file's own start
            let func = P.chain.last().and_then(|c| words(&c.cmd).iter().find_map(|w| w.split_once(':').map(|x| x.1.to_string())));
            let start_defs: Vec<Gid> = P.starts.iter().map(|s| {
                let fi = ix[s];
                let f = &files[fi];
                let di = func.as_deref().filter(|n| !n.is_empty()).and_then(|n| f.def_named(n)).unwrap_or_else(|| f.entry_def());
                (fi, di)
            }).collect();
            for &g in &start_defs {
                walk.visit(g, 0, "start", false, "");
            }
            let mut fr = reach(&files, &P.starts);
            fr.extend(P.chain.iter().flat_map(|c| c.targets.iter().cloned()));
            // files the framework loads for a package the journey passes through
            let in_steps: HashSet<usize> = walk.steps.iter().map(|s| s.file).collect();
            let near: HashSet<&str> = in_steps.iter().flat_map(|&i| std::iter::once(files[i].path.as_str()).chain(files[i].uses.iter().map(|u| u.as_str()))).collect();
            let loaded: Vec<usize> = conv.iter().filter(|(a, m)| near.contains(a.as_str()) && !in_steps.contains(&ix[m])).map(|(_, m)| ix[m]).collect::<BTreeSet<_>>().into_iter().take(12).collect();
            for m in loaded {
                walk.loads(m);
            }
            let steps = walk.steps;
            // web endpoints of the files this program loads
            let mut routes = vec![];
            // a shell script that starts something else does not answer its requests
            let shell_only = P.starts.iter().all(|s| files[ix[s]].lang.name == "Shell");
            for p in fr.iter().filter(|p| !shell_only && lay(&files, p) != "tests") {
                let fi = ix[p];
                let f = &files[fi];
                let mut found: Vec<(String, String, usize, Vec<Gid>, Vec<Gid>)> = vec![];
                // the full path: the prefixes of the routers it is mounted under, the class it is written in, then its own
                let full = |recv: &str, at: usize, class: &str, path: &str| -> String {
                    let up = mounts.prefix_at(fi, at, recv);
                    if up.is_empty() && class.is_empty() { path.to_string() } else { crate::mounts::join_path(&[&up, class, path]) }
                };
                for (di, d) in f.defs.iter().enumerate().filter(|(_, d)| d.kind == Kind::Func) {
                    if let Some((verb, path)) = &d.route {
                        let class = d.parent.map(|p| f.defs[p].prefix.as_str()).unwrap_or("");
                        found.push((verb.clone(), full(&d.route_on, d.start, class, path), d.line, vec![(fi, di)], vec![]));
                    }
                }
                for r in &f.routes {
                    if f.defs.iter().any(|d| d.route.is_some() && d.start <= r.at && r.at < d.end) {
                        continue; // @app.get("/x") above a function: that function's route, found above
                    }
                    // the handler: a function written inline, else the functions handed to the registering call
                    let inline = f.defs.iter().enumerate().find(|(_, d)| d.kind == Kind::Func && d.start >= r.at && d.end <= r.end).map(|(i, _)| (fi, i));
                    let handed: Vec<Gid> = f.calls.iter().enumerate().filter(|(_, c)| c.is_ref && c.at >= r.at && c.end <= r.end).flat_map(|(ci, _)| targets[fi][ci].clone()).collect();
                    // what is handed over before the last function runs first (auth, validation): middleware
                    let mut pre: Vec<Gid> = vec![];
                    // a function written in place after them (`(req, res) => ...`) is the handler, and has no definition of its own
                    let anon = f.calls.iter().filter(|c| c.is_ref && c.at >= r.at && c.end <= r.end).map(|c| c.end).max()
                        .and_then(|e| f.text.get(e..r.end))
                        .is_some_and(|t| t.contains("=>") || t.contains("function") || t.contains("lambda"));
                    let mut hs = match inline {
                        Some(g) => vec![g],
                        None if anon && r.verb != "ANY" => {
                            pre = handed;
                            vec![]
                        }
                        None => match handed.iter().rposition(|g| files[g.0].defs[g.1].kind == Kind::Func) {
                            Some(i) => {
                                pre = handed[..i].to_vec();
                                vec![handed[i]]
                            }
                            // `get('/x', auth.optional, (req, res) => ...)`: the module is middleware, the calls inside are the handler
                            None if r.verb != "ANY" => {
                                pre = handed;
                                vec![]
                            }
                            None => handed,
                        },
                    };
                    if hs.is_empty() {
                        // `[UserController::class, 'index']`: the one method of that class (or module) with that name;
                        // `View.as_view()`: every verb method the class defines
                        if let Some((class, func)) = crate::parse::handler_of(&r.handler).filter(|(c, _)| !c.is_empty()) {
                            let named: Vec<Gid> = files.iter().enumerate().flat_map(|(i, g)| {
                                let module = crate::resolve::basename(&g.path).split('.').next().unwrap_or("").to_string();
                                let (class, func) = (class.clone(), func.clone());
                                g.defs.iter().enumerate()
                                    .filter(move |(_, d)| {
                                        d.kind == Kind::Func
                                            && if func.is_empty() { VERB_METHODS.contains(&d.name.as_str()) } else { d.name == func }
                                            && (d.owner.as_deref() == Some(class.as_str()) || module == class)
                                    })
                                    .map(move |(j, _)| (i, j))
                            }).collect();
                            if func.is_empty() && named.iter().all(|g| g.0 == named[0].0) || named.len() == 1 {
                                hs.extend(named);
                            }
                        }
                    }
                    if hs.is_empty() {
                        // `(req, res) => {...}` written in place: the calls inside it
                        for (ci, c) in f.calls.iter().enumerate() {
                            if c.at > r.at && c.at < r.end && c.def == r.def {
                                for t in &targets[fi][ci] {
                                    if !hs.contains(t) && !pre.contains(t) {
                                        hs.push(*t);
                                    }
                                }
                            }
                        }
                    }
                    found.push((r.verb.clone(), full(&r.recv, r.at, "", &r.path), r.line, hs, pre));
                }
                for (verb, path, line, hs, pre) in found {
                    if routes.len() >= MAX_ROUTES {
                        break;
                    }
                    let mut w = Walk::new(&ctx, MAX_ROUTE_STEPS);
                    for g in pre {
                        if files[g.0].defs[g.1].kind == Kind::Func {
                            w.visit(g, 0, "middleware", false, "");
                        } else {
                            w.middleware(g);
                        }
                    }
                    for g in hs {
                        w.visit(g, 0, "start", false, "");
                    }
                    let mut io: BTreeSet<usize> = BTreeSet::new();
                    for s in &w.steps {
                        io.extend(step_io(&ctx, s).iter().map(|k| crate::tables::io_order(k)));
                    }
                    routes.push((json!({"verb": verb, "path": path, "f": p, "line": line,
                        "io": io.iter().map(|i| IO[*i].key).collect::<Vec<_>>(), "steps": steps_json(&ctx, &w.steps)}), w.steps));
                }
            }
            // inputs and outputs: the files of the steps that do them, nearest the start first
            let mut io_files: BTreeMap<usize, Vec<String>> = BTreeMap::new();
            let all_steps = steps.iter().chain(routes.iter().flat_map(|r| r.1.iter()));
            for s in all_steps {
                for k in &step_io(&ctx, s) {
                    let v = io_files.entry(crate::tables::io_order(k)).or_default();
                    if !v.contains(&files[s.file].path) && v.len() < 4 {
                        v.push(files[s.file].path.clone());
                    }
                }
            }
            let mut parts: Vec<String> = vec![];
            for s in steps.iter().chain(routes.iter().flat_map(|r| r.1.iter())) {
                let u = &unit_of[&files[s.file].path];
                if !parts.contains(u) {
                    parts.push(u.clone());
                }
            }
            // the map's arrows: a step's part to the part of each step nested right under it
            let mut edges: BTreeMap<(&str, &str), usize> = BTreeMap::new();
            for list in std::iter::once(&steps).chain(routes.iter().map(|r| &r.1)) {
                let mut above: Vec<&Step> = vec![];
                for s in list.iter().filter(|s| s.kind != "more") {
                    while above.last().is_some_and(|a| a.depth >= s.depth) {
                        above.pop();
                    }
                    if let Some(a) = above.last() {
                        let (ua, ub) = (unit_of[&files[a.file].path].as_str(), unit_of[&files[s.file].path].as_str());
                        if ua != ub {
                            *edges.entry((ua, ub)).or_default() += 1;
                        }
                    }
                    above.push(s);
                }
            }
            let edges: Vec<Value> = edges.into_iter().map(|((a, b), n)| json!([a, b, n])).collect();
            seen_units.extend(fr.iter().map(|p| unit_of[p].clone()));
            seen_units.extend(parts.iter().cloned());
            for p in &P.also {
                seen_units.insert(unit_of[p].clone());
            }
            for c in &P.chain {
                for s in std::iter::once(&c.file).chain(c.targets.iter()) {
                    if let Some(u) = unit_of.get(s) {
                        seen_units.insert(u.clone());
                    }
                }
            }

            // the story in one paragraph
            let mut text = String::new();
            if !P.chain.is_empty() {
                // how it is started: Dockerfile runs entrypoint.py, which runs `gunicorn app.wsgi`
                let mut st: Vec<String> = P.chain[..P.chain.len() - 1].iter().filter_map(|c| c.targets.first().cloned()).collect();
                let last = P.chain.last().unwrap();
                if !last.cmd.is_empty() {
                    st.push(format!("`{}`", short(&last.cmd)));
                }
                text = basename(&P.chain[0].file).to_string();
                if !st.is_empty() {
                    text += &format!(" runs {}", st.join(", which runs "));
                }
                if !last.pages.is_empty() {
                    text += &format!("{} sends web requests to {}", if st.is_empty() { "" } else { ", and" }, last.pages[..last.pages.len().min(2)].join(", "));
                }
                text += ". ";
            }
            let first: Vec<String> = steps.iter().filter(|s| s.depth == 0 && s.kind == "start").map(|s| {
                let fname = basename(&files[s.file].path);
                if s.label == fname || s.def == Some(0) { s.label.clone() } else { format!("{} in {}", s.label, fname) }
            }).collect();
            let first = if first.len() > 3 { format!("{} and {} more", first[..2].join(", "), first.len() - 2) } else { both(&first) };
            if entry_kind == "Entry points" {
                text += &format!("It starts at {first}");
            } else {
                text += &format!("People using this library start from {first}");
            }
            let mut next: Vec<String> = vec![];
            for s in steps.iter().filter(|s| s.depth == 1 && s.kind != "more") {
                let l = s.label.clone();
                if !next.contains(&l) {
                    next.push(l);
                }
            }
            if !next.is_empty() {
                let shown: Vec<String> = next.iter().take(5).cloned().collect();
                let more = if next.len() > 5 { format!(" and {} more", next.len() - 5) } else { String::new() };
                text += &format!(", which goes on to {}{}", if more.is_empty() { both(&shown) } else { shown.join(", ") }, more);
            }
            text += ".";
            if !routes.is_empty() {
                let n = routes.len();
                text += &format!(" It answers {n} {} of web request.", if n == 1 { "kind" } else { "kinds" });
            }
            let say = |way: &str| -> Vec<String> {
                let mut v: Vec<String> = vec![];
                for i in io_files.keys() {
                    let io = &IO[*i];
                    if io.way == way || io.way == "both" {
                        let l = io_label[io.key].to_lowercase();
                        if !v.contains(&l) {
                            v.push(l);
                        }
                    }
                }
                v
            };
            let (ins, outs) = (say("in"), say("out"));
            if !ins.is_empty() {
                text += &format!(" It takes in {}", both(&ins));
            }
            if !outs.is_empty() {
                text += &format!("{} {}", if ins.is_empty() { " It gives out" } else { ", and gives out" }, both(&outs));
            }
            if !ins.is_empty() || !outs.is_empty() {
                text += ".";
            }
            let mut chain_files: Vec<String> = P.chain.iter().map(|c| c.file.clone()).filter(|f| ix.contains_key(f)).collect();
            chain_files.extend(P.also.iter().cloned());
            let io_map: Map<String, Value> = io_files.iter().map(|(i, fs)| (IO[*i].key.to_string(), json!(fs))).collect();
            out.push(json!({
                "name": P.name, "command": short(&P.cmd), "narrative": text, "entries": P.starts, "chain": chain_files,
                "steps": steps_json(&ctx, &steps), "routes": routes.into_iter().map(|r| r.0).collect::<Vec<_>>(),
                "parts": parts, "edges": edges, "io": io_map,
            }));
        }
        let mut unreached: Vec<&String> = ulayer.keys().filter(|u| !seen_units.contains(*u) && ulayer[*u] != "tests").collect();
        unreached.sort_by_key(|u| (ulayer[*u] == "config", std::cmp::Reverse(nodes[*u].loc), u.to_string()));
        flow = json!({"kind": entry_kind, "processes": out, "unreached": unreached});
    }

    // ---- repository summary
    let mut ext_files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut ext_spec: HashMap<String, String> = HashMap::new();
    for f in &files {
        for (nm, spec) in &f.ext {
            ext_files.entry(nm.clone()).or_default().insert(f.path.clone());
            ext_spec.entry(nm.clone()).or_insert_with(|| spec.clone());
        }
    }
    for nm in &deps {
        // what the dependency files name (gems and composer packages are rarely imported by name)
        let last = nm.rsplit('/').next().unwrap_or(nm);
        if cat_of(nm).is_some() && !ext_files.contains_key(nm)
            && !ext_files.keys().any(|k| cat_of(k).is_some() && k.rsplit('/').next() == Some(last))
        {
            ext_files.entry(nm.clone()).or_default().insert(String::new());
            ext_spec.entry(nm.clone()).or_insert_with(|| nm.clone());
        }
    }
    for P in &procs {
        // the tool a process is run with (gunicorn, celery ...) is part of the stack too
        for c in &P.chain {
            let tool = basename(words(&c.cmd).first().copied().unwrap_or("")).to_string();
            if cat_of(&tool).is_some() {
                ext_files.entry(tool.clone()).or_default().insert(c.file.clone());
                ext_spec.entry(tool.clone()).or_insert(tool);
            }
        }
    }
    let mut ranked: Vec<&String> = ext_files.keys().collect();
    ranked.sort_by_key(|k| (std::cmp::Reverse(ext_files[*k].len()), k.to_string()));
    let cat = |nm: &str| cat_of(&ext_spec[nm]).or_else(|| cat_of(nm));
    let mut catnames: HashMap<&str, Vec<&String>> = HashMap::new();
    for nm in &ranked {
        if let Some(c) = cat(nm) {
            catnames.entry(c).or_default().push(nm);
        }
    }
    let stack: Vec<Value> = EXT_CATS.iter().filter_map(|(c, lab, _)| catnames.get(c).map(|v| json!({"label": lab, "items": v.iter().take(6).collect::<Vec<_>>()}))).collect();
    let libs: Vec<&&String> = ranked.iter().filter(|nm| cat(nm).is_none()).take(10).collect();
    static CLI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(argparse|click|typer|commander|yargs|cobra|clap|optparse)\b").unwrap());
    let cli_hint = entries.iter().any(|p| CLI.is_match(&files[ix[p]].text));
    let has = |c: &str| catnames.contains_key(c);
    let kind = if entry_kind == "Public API" {
        "library"
    } else if has("web") && has("ui") {
        "full-stack app"
    } else if has("web") {
        "backend service"
    } else if has("ui") {
        "frontend app"
    } else if cli_hint {
        "command-line tool"
    } else {
        "application"
    };
    let mut langs: Tally<&str> = Tally::default();
    for f in &files {
        langs.add(f.lang.name, f.loc);
    }
    let lc = langs.most_common();
    let main_lang = lc.iter().find(|(l, _)| !crate::lang::by_name(l).markup).or(lc.first()).map(|x| x.0).unwrap_or("");
    let mut summary = readme_dir.get("").map(|r| read_para(&project.read(r))).unwrap_or_default();
    if summary.is_empty() {
        summary = format!("A {main_lang} {kind} with {} main parts.", units.len());
    }
    let routes = files.iter().filter(|f| f.layer != Some("tests")).flat_map(|f| f.symbols.iter()).filter(|s| s.k == "route").count();
    let colors: Map<String, Value> = lc.iter().map(|(l, _)| (l.to_string(), json!(LANGS.iter().find(|x| x.name == *l).unwrap().color))).collect();
    let repo = json!({
        "name": name, "summary": summary, "kind": format!("{main_lang} {kind}"), "files": files.len(),
        "loc": lc.iter().map(|x| x.1).sum::<usize>(), "parts": units.len(), "deps": ext_files.len(),
        "langs": lc.iter().take(6).map(|(l, c)| json!([l, c])).collect::<Vec<_>>(), "routes": routes, "stack": stack,
        "libs": libs, "entries": entries, "generated": today(),
        "runs": if entry_kind == "Entry points" { procs.iter().map(|P| P.name.clone()).collect::<Vec<_>>() } else { vec![] },
        "colors": colors,
        "scip": scip_index.as_ref().map(|s| json!({"documents": s.documents(), "calls": scip_hits})),
    });
    let nodes_json: Map<String, Value> = nodes.into_iter().map(|(id, n)| (id, n.to_json(&files, &ix))).collect();
    let io: Map<String, Value> = IO.iter().map(|i| (i.key.to_string(), json!([i.label, i.way]))).collect();
    Ok(json!({"repo": repo, "layers": layers_out, "nodes": nodes_json, "flow": flow, "io": io}))
}

enum NodeReq {
    File(String, String),
    Dir(String, String, String),
}

struct FileInfo {
    lang: &'static str,
    entry: bool,
    ext: Vec<String>,
}

struct Node {
    id: String,
    name: String,
    kind: &'static str,
    path: String,
    loc: usize,
    langs: Vec<(String, usize)>,
    children: Vec<String>,
    edges: Vec<Value>,
    uses: Vec<String>,
    used_by: Vec<String>,
    parent: Option<String>,
    layer: Option<&'static str>,
    desc: String,
    io: Vec<&'static str>,
    files: Vec<String>,
    unit: bool,
    file: Option<FileInfo>,
}

impl Node {
    fn to_json(&self, files: &[File], ix: &HashMap<String, usize>) -> Value {
        let mut o = json!({
            "id": self.id, "name": self.name, "kind": self.kind, "path": self.path, "files": self.files.len(), "loc": self.loc,
            "langs": self.langs.iter().map(|(l, c)| json!([l, c])).collect::<Vec<_>>(), "children": self.children,
            "edges": self.edges, "uses": self.uses, "usedBy": self.used_by, "parent": self.parent, "layer": self.layer,
            "desc": self.desc, "io": self.io,
        });
        let m = o.as_object_mut().unwrap();
        if self.unit {
            m.insert("unit".into(), json!(true));
        }
        if let Some(fi) = &self.file {
            let f = &files[ix[&self.id]];
            let symbols: Vec<Value> = f.symbols.iter().map(|s| {
                let mut v = json!({"k": s.k, "n": s.n, "sig": s.sig, "doc": s.doc, "line": s.line});
                if let Some(ms) = &s.members {
                    v.as_object_mut().unwrap().insert("members".into(), json!(ms));
                }
                v
            }).collect();
            m.insert("lang".into(), json!(fi.lang));
            m.insert("symbols".into(), json!(symbols));
            m.insert("entry".into(), json!(fi.entry));
            m.insert("ext".into(), json!(fi.ext));
        }
        o
    }
}
