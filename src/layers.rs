//! Layers (what a file is for, from its path and libraries), parts (folders grouped into boxes), and files a
//! framework loads by convention.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use crate::parse::File;
use crate::resolve::{basename, dirname};
use crate::tables::{cat_of, toks, CAT_LAYER, ROUTE_HINT, TOKEN_LAYER, WEAK_TESTS};

/// A counter that remembers the order keys were first seen, so ties break the same way every run.
#[derive(Default, Clone)]
pub struct Tally<K: Eq + std::hash::Hash + Clone> {
    order: Vec<K>,
    n: HashMap<K, usize>,
}

impl<K: Eq + std::hash::Hash + Clone> Tally<K> {
    pub fn add(&mut self, k: K, by: usize) {
        if !self.n.contains_key(&k) {
            self.order.push(k.clone());
        }
        *self.n.entry(k).or_insert(0) += by;
    }
    pub fn get(&self, k: &K) -> usize {
        self.n.get(k).copied().unwrap_or(0)
    }
    /// most first; equal counts in the order first seen
    pub fn most_common(&self) -> Vec<(K, usize)> {
        let mut v: Vec<(K, usize)> = self.order.iter().map(|k| (k.clone(), self.n[k])).collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        v
    }
    /// every key with its count, in the order first seen
    pub fn items(&self) -> Vec<(K, usize)> {
        self.order.iter().map(|k| (k.clone(), self.n[k])).collect()
    }
    pub fn top(&self) -> Option<K> {
        self.most_common().into_iter().next().map(|x| x.0)
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

pub fn stem(p: &str) -> &str {
    let b = basename(p);
    b.rsplit_once('.').map(|x| x.0).unwrap_or(b)
}

pub fn classify(f: &File, fullstack: bool) -> Option<&'static str> {
    let parts: Vec<&str> = f.path.split('/').collect();
    let mut segs = vec![stem(&f.path)]; // file stem first, then folders deepest -> shallowest
    segs.extend(parts[..parts.len() - 1].iter().rev());
    let depth = segs.len() - 1;
    let all: Vec<Vec<String>> = segs
        .iter()
        .enumerate()
        .map(|(i, s)| toks(s).into_iter().filter(|t| !WEAK_TESTS.contains(&t.as_str()) || i == 0 || depth - i < 2).collect())
        .collect();
    if all.iter().flatten().any(|t| TOKEN_LAYER.get(t.as_str()) == Some(&"tests")) {
        return Some("tests");
    }
    if all[1..].iter().flatten().any(|t| ["config", "configs", "conf"].contains(&t.as_str())) {
        return Some("config"); // config/database.php is a setting
    }
    for ts in &all {
        for t in ts.iter().rev() {
            if let Some(ly) = TOKEN_LAYER.get(t.as_str()) {
                if t == "client" {
                    return Some(if fullstack { "interface" } else { "integration" });
                }
                if let Some((_, l)) = f.lang.stem_layers.iter().find(|(w, _)| w == t) {
                    return Some(l);
                }
                return Some(ly);
            }
        }
    }
    let cats: BTreeSet<&str> = f.ext.iter().filter_map(|(_, s)| cat_of(s)).collect();
    if let Some((_, layer)) = CAT_LAYER.iter().find(|(c, _)| cats.contains(c)) {
        return Some(layer);
    }
    if f.lang.script_blocks || (f.lang.grammar.is_some_and(|g| g == "javascript" || g == "typescript") && (f.path.ends_with(".jsx") || f.path.ends_with(".tsx"))) {
        return Some("interface");
    }
    if let Some(l) = f.lang.default_layer {
        return Some(l);
    }
    // endpoints the language found; raw text only where the language has no exact route finder of its own
    if f.symbols.iter().any(|s| s.k == "route") || (f.lang.regex_routes && ROUTE_HINT.is_match(&f.code)) {
        return Some("api");
    }
    None
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub kind: &'static str,
    pub id: String,
    pub path: String,
    pub files: Vec<String>,
    pub layer: Option<&'static str>,
    pub multi: bool,
}

pub struct Units {
    pub units: Vec<Unit>,
    pub tree: BTreeMap<String, BTreeSet<String>>,
    pub loose: BTreeMap<String, Vec<String>>,
    pub sub: BTreeMap<String, Vec<String>>,
}

impl Units {
    /// skip wrapper folders that contain just one sub-folder
    pub fn squash(&self, d: &str) -> String {
        let mut d = d.to_string();
        while self.loose.get(&d).is_none_or(|l| l.is_empty()) && self.tree.get(&d).is_some_and(|t| t.len() == 1) {
            d = self.tree[&d].iter().next().unwrap().clone();
        }
        d
    }
}

pub fn build_units(files: &[File], small: bool) -> Units {
    let by: HashMap<&str, &File> = files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut u = Units { units: vec![], tree: BTreeMap::new(), loose: BTreeMap::new(), sub: BTreeMap::new() };
    for f in files {
        let d = dirname(&f.path);
        u.loose.entry(d.to_string()).or_default().push(f.path.clone());
        let parts: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
        for i in 0..=parts.len() {
            let anc = parts[..i].join("/");
            u.sub.entry(anc.clone()).or_default().push(f.path.clone());
            if i > 0 {
                u.tree.entry(parts[..i - 1].join("/")).or_default().insert(anc);
            }
        }
    }
    let dominant = |d: &str| -> (Option<&'static str>, f64) {
        let mut c: Tally<&'static str> = Tally::default();
        let mut total = 0;
        for p in &u.sub[d] {
            let f = by[p.as_str()];
            total += f.loc;
            if let Some(l) = f.layer {
                c.add(l, f.loc);
            }
        }
        match c.most_common().first() {
            Some((k, v)) => (Some(k), *v as f64 / total.max(1) as f64),
            None => (None, 0.0),
        }
    };
    let mut units = vec![];
    let mut stack = vec![(String::new(), 0usize, false)];
    // depth-first, children in order, the folder's own files after its sub-folders (as a recursive visit would)
    while let Some((d, depth, done)) = stack.pop() {
        let d = if done { d } else { u.squash(&d) };
        let subs: Vec<String> = u.tree.get(&d).map(|t| t.iter().cloned().collect()).unwrap_or_default();
        let lo = u.loose.get(&d).cloned().unwrap_or_default();
        if done {
            let mut groups: Vec<(&'static str, Vec<String>)> = vec![];
            for p in &lo {
                let ly = by[p.as_str()].layer.unwrap_or("logic");
                match groups.iter_mut().find(|g| g.0 == ly) {
                    Some(g) => g.1.push(p.clone()),
                    None => groups.push((ly, vec![p.clone()])),
                }
            }
            let multi = groups.len() > 1;
            for (ly, ps) in groups {
                if ps.len() <= 3 || lo.len() <= 24 {
                    units.extend(ps.into_iter().map(|p| Unit { kind: "file", id: p.clone(), path: p.clone(), files: vec![p], layer: None, multi: false }));
                } else {
                    units.push(Unit { kind: "group", id: format!("{d}#{ly}"), path: d.clone(), files: ps, layer: Some(ly), multi });
                }
            }
            continue;
        }
        let n = u.sub[&d].len();
        let (top, share) = dominant(&d);
        let calm = matches!(top, Some("tests" | "config")) && share >= 0.7; // keep test/config folders as one box
        let expand = depth == 0
            || (!calm && depth < 3 && ((small && n >= 2) || (!subs.is_empty() && n > 8 && (subs.len() >= 3 || share < 0.7))));
        if !expand {
            units.push(Unit { kind: "dir", id: d.clone(), path: d.clone(), files: u.sub[&d].clone(), layer: None, multi: false });
            continue;
        }
        stack.push((d.clone(), depth, true));
        for s in subs.into_iter().rev() {
            stack.push((s, depth + 1, false));
        }
    }
    if !small && units.len() < 8 {
        return build_units(files, true); // tiny repo: open the folders up so there is something to explore
    }
    u.units = units;
    u
}

/// Files a framework loads by name or by scanning a folder, which no code imports:
/// C1 a name repeated in sibling packages (apps.py, migrations/), C2 a folder of look-alike files (pages/, commands/).
/// -> ({anchor file: {member files}}, [members with no anchor]). The anchor is the package file that stands for
/// the package, so the members join any journey that reaches it.
pub fn conventions(files: &[File]) -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
    static NUMBERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[vV]?\d{3,}").unwrap());
    const PKG: &[&str] = &["__init__", "index", "mod"];
    let by: HashMap<&str, &File> = files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut live: Vec<&str> = files.iter().filter(|f| f.layer != Some("tests")).map(|f| f.path.as_str()).collect();
    live.sort();
    let mut users: HashMap<&str, usize> = HashMap::new();
    for p in &live {
        for q in &by[p].uses {
            *users.entry(q.as_str()).or_insert(0) += 1;
        }
    }
    let mut own: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut under: Vec<(&str, Vec<&str>)> = vec![]; // in first-seen order
    let mut under_ix: HashMap<&str, usize> = HashMap::new();
    for p in &live {
        own.entry(dirname(p)).or_default().push(p);
        let mut d = dirname(p);
        loop {
            let i = *under_ix.entry(d).or_insert_with(|| {
                under.push((d, vec![]));
                under.len() - 1
            });
            under[i].1.push(p);
            if d.is_empty() {
                break;
            }
            d = dirname(d);
        }
    }
    let free = |ps: &[&str]| !ps.iter().any(|p| users.get(p).copied().unwrap_or(0) > 0 || by[p].entry);
    let numbered = |p: &str| NUMBERED.is_match(basename(p));
    let anchor = |d: &str, skip: &[&str]| -> Option<String> {
        let mut d = d;
        while !d.is_empty() {
            let cands: Vec<&&str> = own.get(d).into_iter().flatten().filter(|p| !skip.contains(p)).collect();
            if let Some(best) = cands.into_iter().min_by_key(|p| (!PKG.contains(&stem(p)), std::cmp::Reverse(users.get(**p).copied().unwrap_or(0)), p.to_string())) {
                return Some(best.to_string()); // the package file, else the most used one
            }
            d = dirname(d);
        }
        None
    };
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut loose: BTreeSet<String> = BTreeSet::new();
    let mut add = |d: &str, members: &[&str]| match anchor(d, members) {
        Some(a) => found.entry(a).or_default().extend(members.iter().map(|m| m.to_string())),
        None => loose.extend(members.iter().map(|m| m.to_string())),
    };
    let mut kids: BTreeMap<&str, Vec<&str>> = BTreeMap::new(); // folder -> its sub-folders that hold code
    for (d, _) in &under {
        if !d.is_empty() {
            kids.entry(dirname(d)).or_default().push(d);
        }
    }
    let under_of = |d: &str| under_ix.get(d).map(|&i| under[i].1.clone()).unwrap_or_default();
    for subs in kids.values() {
        // C1
        let mut names: Vec<(String, Vec<(&str, Vec<&str>)>)> = vec![];
        fn push<'x>(names: &mut Vec<(String, Vec<(&'x str, Vec<&'x str>)>)>, k: String, v: (&'x str, Vec<&'x str>)) {
            match names.iter_mut().find(|x| x.0 == k) {
                Some(x) => x.1.push(v),
                None => names.push((k, vec![v])),
            }
        }
        for s in subs {
            for p in own.get(s).into_iter().flatten() {
                if !PKG.contains(&stem(p)) {
                    push(&mut names, basename(p).to_string(), (s, vec![p]));
                }
            }
            for c in kids.get(s).into_iter().flatten() {
                push(&mut names, format!("{}/", basename(c)), (s, under_of(c)));
            }
        }
        for (_, places) in &names {
            let unused: Vec<(&str, Vec<&str>)> = places
                .iter()
                .filter(|(_, ms)| free(ms))
                .map(|(s, ms)| (*s, ms.iter().copied().filter(|m| !numbered(m)).collect()))
                .collect();
            if places.len() >= 2 && unused.len() == places.len() {
                // imported nowhere: the framework finds it by name
                for (s, ms) in &unused {
                    if !ms.is_empty() {
                        add(s, ms);
                    }
                }
            }
        }
    }
    for (d, ps) in &own {
        // C2
        let ext = |p: &str| basename(p).rfind('.').map(|i| basename(p)[i..].to_string()).unwrap_or_default();
        let mut c: Tally<String> = Tally::default();
        for p in ps {
            c.add(ext(p), 1);
        }
        let Some(top) = c.top() else { continue };
        let unused: Vec<&str> = ps.iter().copied().filter(|p| free(&[p]) && ext(p) == top).collect();
        let mut lc: Tally<Option<&str>> = Tally::default();
        for p in &unused {
            lc.add(by[p].layer, 1);
        }
        if !d.is_empty() && unused.len() >= 3 && 2 * unused.len() >= ps.len() && !unused.iter().all(|p| numbered(p))
            && lc.top() != Some(Some("config"))
        {
            let members: Vec<&str> = unused.into_iter().filter(|p| !numbered(p)).collect();
            add(d, &members);
        }
    }
    (found, loose)
}
