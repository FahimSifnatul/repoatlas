//! Links between files and between functions, the same way for every language:
//! paths and module names written in a file, names a file uses that another defines, and calls resolved
//! to the definitions they run (same class, same file, imported files, same package, the only one in the repo).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::finder::Finder;
use crate::lang::{Lang, Resolver, Scope};
use crate::parse::{Call, File, Kind};
use crate::resolve::dirname;
use crate::tables::{cat_of, io_of_cat, io_order, COMMON_METHODS, COMMON_NAMES, IN_STRINGS, IO_RES, PATH_REF, STRING};

/// a definition anywhere in the repo: (file index, definition index)
pub type Gid = (usize, usize);

/// How sure a resolved call is: Exact (an import or a written-out path proves it), Near (same class, file, folder or
/// a name only one definition has), Guess (read off the spelling of the object it is called on).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conf {
    Exact,
    Near,
    Guess,
}

/// Languages that call into each other's files by the same rules.
pub fn family(l: &Lang) -> &'static str {
    match l.resolver {
        Resolver::Js => "js",
        Resolver::Jvm => "jvm",
        Resolver::C => "c",
        _ => l.name,
    }
}

fn ident(s: &str) -> bool {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_$@][\w$]*$").unwrap());
    ID.is_match(s)
}

/// File -> files it names by path (templates, assets, scripts) or by module name in a string ("food.settings",
/// "gunicorn app.wsgi"), plus files defining names it uses where the language needs no import for them.
pub fn link(files: &[File], find: &Finder) -> Vec<BTreeSet<String>> {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z_]\w*").unwrap());
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z][a-z0-9]+[A-Z]\w*").unwrap());
    let mut defs: HashMap<(&str, &str, &str), BTreeSet<&str>> = HashMap::new();
    for f in files {
        if f.lang.scope == Scope::Imports {
            continue;
        }
        let scope = if f.lang.scope == Scope::Folder { dirname(&f.path) } else { "" };
        for d in f.defs.iter().skip(1) {
            if matches!(d.kind, Kind::Func | Kind::Class | Kind::Type) && d.owner.is_none() && d.name.len() >= 3
                && !d.name.starts_with('_') && !COMMON_NAMES.contains(&d.name.as_str())
            {
                defs.entry((family(f.lang), scope, d.name.as_str())).or_default().insert(&f.path);
            }
        }
    }
    files
        .iter()
        .map(|f| {
            let d = dirname(&f.path);
            let mut refs: BTreeSet<String> = BTreeSet::new();
            let toks: BTreeSet<String> = PATH_REF.captures_iter(&f.bare).filter_map(|c| c.ok().map(|c| c[1].to_string())).collect();
            for tok in toks {
                refs.extend(find.path(&tok, d));
            }
            let strs: BTreeSet<&str> = STRING
                .captures_iter(&f.bare)
                .filter_map(|c| c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str()))
                .collect();
            for s in &strs {
                // frameworks wire code together with names in strings
                refs.extend(find.command(s, d, false, None));
                refs.extend(find.pattern(s, d));
            }
            if f.lang.scope != Scope::Imports {
                let scope = if f.lang.scope == Scope::Folder { d } else { "" };
                let mut words: BTreeSet<&str> = WORD.find_iter(&f.code).map(|m| m.as_str()).collect();
                words.extend(strs.iter().flat_map(|s| CAMEL.find_iter(s).map(|m| m.as_str()))); // 'AuthController@login'
                for w in words {
                    if let Some(hit) = defs.get(&(family(f.lang), scope, w)) {
                        if hit.len() == 1 {
                            // a name two files define says nothing
                            refs.extend(hit.iter().map(|s| s.to_string()));
                        }
                    }
                }
            }
            refs.remove(&f.path);
            refs
        })
        .collect()
}

struct Index<'a> {
    files: &'a [File],
    by_path: HashMap<&'a str, usize>,
    /// (family, name) -> top-level functions and classes
    top: HashMap<(&'static str, &'a str), Vec<Gid>>,
    /// (family, class, name) -> methods
    methods: HashMap<(&'static str, &'a str, &'a str), Vec<Gid>>,
    /// (family, name) -> methods of any class
    any_method: HashMap<(&'static str, &'a str), Vec<Gid>>,
    /// (family, name) -> classes
    classes: HashMap<(&'static str, &'a str), Vec<Gid>>,
    /// (family, base) -> classes built on it
    subclasses: HashMap<(&'static str, &'a str), Vec<&'a str>>,
    /// (family, lower-case name) -> class names
    lower: HashMap<(&'static str, String), BTreeSet<&'a str>>,
    /// file -> its module path: "src/graph" for src/graph.rs, "app" for app/__init__.py
    modpath: Vec<String>,
    /// (family, last part of a module path) -> files
    modfiles: HashMap<(&'static str, String), Vec<usize>>,
    /// crate name -> its lib.rs
    crates: HashMap<&'a str, usize>,
    /// file -> the files it reaches through what it imports (built on first use)
    reach: std::cell::RefCell<HashMap<usize, std::rc::Rc<HashSet<usize>>>>,
}

const SELF: &[&str] = &["self", "this", "cls", "$this", "Self", "static", "@"];
const SUPER: &[&str] = &["super", "parent", "base"];
const CTORS: &[&str] = &["__init__", "constructor", "initialize", "new", "init"];

impl<'a> Index<'a> {
    fn new(files: &'a [File], libs: &'a HashMap<String, String>) -> Index<'a> {
        let by_path: HashMap<&str, usize> = files.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
        let modpath: Vec<String> = files.iter().map(|f| module_path(&f.path)).collect();
        let mut modfiles: HashMap<(&'static str, String), Vec<usize>> = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            let last = modpath[i].rsplit('/').next().unwrap_or("").to_string();
            modfiles.entry((family(f.lang), last)).or_default().push(i);
        }
        let crates = libs.iter().filter_map(|(n, p)| by_path.get(p.as_str()).map(|&i| (n.as_str(), i))).collect();
        let mut ix = Index {
            files, by_path, top: HashMap::new(), methods: HashMap::new(), any_method: HashMap::new(), classes: HashMap::new(),
            subclasses: HashMap::new(), reach: Default::default(), lower: HashMap::new(), modpath, modfiles, crates,
        };
        for (fi, f) in files.iter().enumerate() {
            let fam = family(f.lang);
            for (di, d) in f.defs.iter().enumerate().skip(1) {
                match (d.kind, &d.owner) {
                    (Kind::Func, Some(o)) => {
                        ix.methods.entry((fam, o, &d.name)).or_default().push((fi, di));
                        ix.any_method.entry((fam, &d.name)).or_default().push((fi, di));
                    }
                    (Kind::Func, None) => ix.top.entry((fam, &d.name)).or_default().push((fi, di)),
                    (Kind::Class | Kind::Type, _) => {
                        ix.top.entry((fam, &d.name)).or_default().push((fi, di));
                        ix.classes.entry((fam, &d.name)).or_default().push((fi, di));
                        ix.lower.entry((fam, d.name.to_lowercase())).or_default().insert(&d.name);
                        for b in &d.bases {
                            ix.subclasses.entry((fam, b)).or_default().push(&d.name);
                        }
                    }
                    _ => {}
                }
            }
        }
        ix
    }

    /// the files `fi` can see: itself and everything its imports lead to
    fn reaches(&self, fi: usize) -> std::rc::Rc<HashSet<usize>> {
        if let Some(r) = self.reach.borrow().get(&fi) {
            return r.clone();
        }
        let mut seen: HashSet<usize> = [fi].into();
        let mut todo = vec![fi];
        while let Some(i) = todo.pop() {
            for u in &self.files[i].uses {
                if let Some(&j) = self.by_path.get(u.as_str()) {
                    if seen.insert(j) {
                        todo.push(j);
                    }
                }
            }
        }
        let r = std::rc::Rc::new(seen);
        self.reach.borrow_mut().insert(fi, r.clone());
        r
    }

    /// code outside the tests never runs a test file's definitions
    fn live(&self, g: Gid, caller: usize) -> bool {
        self.files[caller].layer == Some("tests") || self.files[g.0].layer != Some("tests")
    }

    fn def(&self, g: Gid) -> &'a crate::parse::Def {
        &self.files[g.0].defs[g.1]
    }

    /// the class a definition is written in
    fn owner_of(&self, fi: usize, di: usize) -> Option<&'a str> {
        let defs = &self.files[fi].defs;
        let mut i = Some(di);
        while let Some(j) = i {
            let d = &defs[j];
            if let Some(o) = &d.owner {
                return Some(o);
            }
            if matches!(d.kind, Kind::Class | Kind::Impl) {
                return Some(&d.name);
            }
            i = d.parent;
        }
        None
    }

    /// a class -> what calling it runs: its constructor, else the class itself
    fn construct(&self, g: Gid) -> Gid {
        let d = self.def(g);
        if !matches!(d.kind, Kind::Class) {
            return g;
        }
        let fam = family(self.files[g.0].lang);
        for c in CTORS.iter().copied().chain([d.name.as_str()]) {
            if let Some(m) = self.methods.get(&(fam, d.name.as_str(), c)) {
                return m.iter().copied().find(|m| m.0 == g.0).unwrap_or(m[0]);
            }
        }
        g
    }

    /// methods called `name` of class `cls` (searching up its bases), plus the overrides in classes built on it
    fn method(&self, fam: &'static str, cls: &str, name: &str, overrides: bool) -> Vec<Gid> {
        let mut out = vec![];
        let mut todo = vec![cls.to_string()];
        let mut seen = HashSet::new();
        while let Some(c) = todo.pop() {
            if !seen.insert(c.clone()) || seen.len() > 8 {
                continue;
            }
            if let Some(m) = self.methods.get(&(fam, c.as_str(), name)) {
                out.extend(m.iter().copied());
                break;
            }
            for k in self.classes.get(&(fam, c.as_str())).into_iter().flatten() {
                todo.extend(self.def(*k).bases.iter().cloned());
            }
        }
        if overrides && !out.is_empty() {
            for sub in self.subclasses.get(&(fam, cls)).into_iter().flatten() {
                if let Some(m) = self.methods.get(&(fam, *sub, name)) {
                    out.extend(m.iter().copied());
                }
            }
        }
        out.sort();
        out.dedup();
        out.truncate(6);
        out
    }

    /// top-level definitions called `name` in the given files (a function nested in another counts only in its own file)
    fn in_files(&self, fam: &'static str, name: &str, files: &HashSet<usize>, funcs_only: bool, home: usize) -> Vec<Gid> {
        self.top
            .get(&(fam, name))
            .into_iter()
            .flatten()
            .copied()
            .filter(|g| files.contains(&g.0) && (!funcs_only || self.def(*g).kind == Kind::Func))
            .filter(|g| g.0 == home || self.def(*g).parent == Some(0))
            .collect()
    }

    /// prefer the class written in this file, then one in a file it imports
    fn class_named(&self, fam: &'static str, name: &str, fi: usize, imported: &HashSet<usize>) -> Option<Gid> {
        let ks = self.classes.get(&(fam, name))?;
        ks.iter().copied().find(|g| g.0 == fi).or_else(|| ks.iter().copied().find(|g| imported.contains(&g.0))).or(ks.first().copied())
    }

    /// `name` as written in file `fi` through an import (`from a import name`, `use a::name`), following re-exports
    fn via_import(&self, fi: usize, name: &str, depth: usize) -> Vec<Gid> {
        if depth > 3 {
            return vec![];
        }
        for (local, remote, path) in &self.files[fi].bound_names {
            if local != name {
                continue;
            }
            if let Some(&j) = self.by_path.get(path.as_str()) {
                let v = self.defined_in(j, remote, depth + 1);
                if !v.is_empty() {
                    return v;
                }
            }
        }
        vec![]
    }

    /// what `name` is in file `j`: a function or class it defines at its top level, else what it re-exports under that name
    fn defined_in(&self, j: usize, name: &str, depth: usize) -> Vec<Gid> {
        let fam = family(self.files[j].lang);
        let mut v: Vec<Gid> = self.top.get(&(fam, name)).into_iter().flatten().copied()
            .filter(|g| g.0 == j && self.def(*g).parent == Some(0)).collect();
        v.sort();
        match v.first() {
            Some(g) => vec![self.construct(*g)],
            None => self.via_import(j, name, depth),
        }
    }

    /// `a::b::f()`, `pkg.mod.f()`, `Class::f()`, `fs::File::open()`: follow the path written out.
    /// Some(empty) = it leads outside the repo or nowhere; None = not a path of modules, try the other rules.
    fn qualified(&self, fi: usize, recv: &str, name: &str, imported: &HashSet<usize>) -> Option<Vec<Gid>> {
        static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_$][\w$]*(?:(?:::|\.)[A-Za-z_$][\w$]*)*$").unwrap());
        if !PATH.is_match(recv) {
            return None;
        }
        let f = &self.files[fi];
        let fam = family(f.lang);
        let segs: Vec<&str> = recv.split("::").flat_map(|x| x.split('.')).collect();
        let root = segs[0];
        if SELF.contains(&root) || SUPER.contains(&root) {
            return None;
        }
        if f.lang.name == "Rust" && ["std", "core", "alloc"].contains(&root) {
            return Some(vec![]);
        }
        let rooted = ["crate", "self", "super"].contains(&root);
        let mods: Vec<&str> = segs.iter().copied().skip_while(|x| ["crate", "self", "super"].contains(x)).collect();
        let bound = f.bound.iter().find(|(a, _)| a == root).and_then(|(_, p)| self.by_path.get(p.as_str()).copied());
        let seen = |i: usize| {
            let p = &self.files[i].path;
            let st = crate::layers::stem(p);
            st == root || dirname(p).split('/').any(|d| d == root) // `import api.v1.users`: api is a folder above users.py
        };
        let crate_lib = self.crates.get(root).copied().filter(|_| !rooted);
        let internal = rooted || bound.is_some() || crate_lib.is_some() || f.mods.iter().any(|m| m == root) || imported.iter().any(|&i| seen(i));
        if !internal {
            // an imported library (os.path.join, fs::File::open, http.Get): not the repo's
            return if f.aliases.iter().any(|a| a == root) { Some(vec![]) } else { None };
        }
        let last = *segs.last().unwrap();
        if segs.len() > 1 && self.classes.contains_key(&(fam, last)) {
            let v = self.method(fam, last, name, false);
            if !v.is_empty() {
                return Some(v);
            }
            if CTORS.contains(&name) {
                return self.class_named(fam, last, fi, imported).map(|k| vec![self.construct(k)]);
            }
            return Some(vec![]);
        }
        let want: Vec<&str> = if crate_lib.is_some() { segs[1..].to_vec() } else { mods };
        let cands: Vec<usize> = if let Some(b) = bound.filter(|_| segs.len() == 1) {
            vec![b]
        } else if want.is_empty() {
            crate_lib.map(|l| vec![l]).unwrap_or_else(|| vec![fi])
        } else {
            let target = want.join("/");
            let mut v: Vec<usize> = self.modfiles.get(&(fam, want.last().unwrap().to_string())).into_iter().flatten().copied()
                .filter(|&i| self.modpath[i] == target || self.modpath[i].ends_with(&format!("/{target}"))).collect();
            v.sort_by_key(|&i| (!imported.contains(&i), self.files[i].path.clone()));
            v
        };
        let hit = cands.iter().map(|&i| self.defined_in(i, name, 0)).find(|v| !v.is_empty());
        match hit {
            Some(v) => Some(v),
            None if segs.len() > 1 => Some(vec![]),
            None => None,
        }
    }

    /// a function handed to another call: `Thread(target=run)`, `app.use(routes)`, `for_each(parse::parse)`
    fn resolve_ref(&self, fi: usize, c: &Call, imported: &HashSet<usize>, folder: &HashSet<usize>) -> Vec<Gid> {
        let f = &self.files[fi];
        let fam = family(f.lang);
        let name = c.name.as_str();
        let recv = c.recv.as_str();
        let here: HashSet<usize> = [fi].into();
        let one = |v: Vec<Gid>| -> Vec<Gid> {
            // the nearest file wins; several in one file (overloads) collapse to the first
            let mut v = v;
            v.sort_by_key(|g| (g.0 != fi, g.1));
            v.truncate(1);
            v
        };
        if recv.is_empty() {
            // a class handed over (`session.get(Item, id)`) runs nothing yet
            let v: Vec<Gid> = self.via_import(fi, name, 0).into_iter().filter(|g| self.def(*g).kind == Kind::Func).collect();
            if !v.is_empty() {
                return v;
            }
        } else if SELF.contains(&recv) {
            // Thread(target=self.run)
            return self.owner_of(fi, c.def).map(|o| self.method(fam, o, name, true)).unwrap_or_default();
        } else if let Some(v) = self.qualified(fi, recv, name, imported) {
            if !v.is_empty() || !f.bound.iter().any(|(a, _)| a == recv) {
                return v;
            }
        }
        for scope in [&here, imported, folder] {
            let v = if recv.is_empty() { self.in_files(fam, name, scope, true, fi) } else { vec![] };
            if !v.is_empty() {
                return one(v);
            }
        }
        // a module handed over (`app.use(routes)`), or something of one (`items.router`): a function of that
        // module, else the object its top level builds
        let module = if recv.is_empty() { name } else { recv };
        let file = f.bound.iter().find(|(a, _)| a == module).and_then(|(_, p)| self.by_path.get(p.as_str()).copied());
        if let Some(i) = file {
            let defined: Vec<Gid> = self.top.get(&(fam, name)).into_iter().flatten().copied().filter(|g| g.0 == i).collect();
            // the module itself (`items`, a default import), or a name its top level builds with a call
            let p = &self.files[i].path;
            let is_module = f.lang.resolver == crate::lang::Resolver::Js || norm(crate::layers::stem(p)) == norm(module)
                || norm(crate::resolve::basename(dirname(p))) == norm(module);
            let built = || {
                let re = Regex::new(&format!(r"(?m)^\s*{}\s*(?::[^=\n]+)?=\s*[\w.]+\s*\(", regex::escape(name))).unwrap();
                re.is_match(&self.files[i].code)
            };
            return match (recv.is_empty(), defined.first()) {
                (false, Some(g)) if is_module && self.def(*g).kind == Kind::Func => vec![*g],
                (_, Some(_)) => vec![],           // a class handed over runs nothing yet
                (false, None) if is_module && i != fi => vec![(i, 0)],
                (true, None) if i != fi && (is_module || built()) => vec![(i, 0)],
                _ => vec![],
            };
        }
        if !recv.is_empty() {
            return vec![];
        }
        // a module is a value only in Python and JavaScript; elsewhere `text` handed over is a variable
        if matches!(f.lang.resolver, Resolver::Python | Resolver::Js) && f.aliases.iter().any(|a| a == name) {
            let key = norm(name);
            let hit: Vec<usize> = imported.iter().copied().filter(|&i| {
                let p = &self.files[i].path;
                let st = crate::layers::stem(p);
                norm(st) == key || (["index", "__init__", "mod"].contains(&st) && norm(crate::resolve::basename(dirname(p))) == key)
            }).collect();
            if hit.len() == 1 {
                return vec![(hit[0], 0)];
            }
        }
        vec![]
    }

    fn resolve(&self, fi: usize, c: &Call, imported: &HashSet<usize>, folder: &HashSet<usize>, types: &HashMap<String, String>) -> (Vec<Gid>, Conf) {
        let f = &self.files[fi];
        let fam = family(f.lang);
        let name = c.name.as_str();
        let recv = c.recv.as_str();
        let here: HashSet<usize> = [fi].into();
        let one = |v: Vec<Gid>| -> Vec<Gid> {
            // the nearest file wins; several in one file (overloads) collapse to the first
            let mut v = v;
            v.sort_by_key(|g| (g.0 != fi, g.1));
            v.truncate(1);
            v
        };
        if c.is_ref {
            return (self.resolve_ref(fi, c, imported, folder), Conf::Near);
        }
        let owner = self.owner_of(fi, c.def);
        // 1. self.x() / this.x(): the class it is written in (and the classes that override it)
        if SELF.contains(&recv) || (recv.is_empty() && f.lang.implicit_this && owner.is_some()) {
            if let Some(o) = owner {
                let v = self.method(fam, o, name, true);
                if !v.is_empty() {
                    return (v, Conf::Near);
                }
            }
            if !recv.is_empty() {
                return (vec![], Conf::Near);
            }
        }
        if SUPER.contains(&recv) || recv.starts_with("super(") {
            if let Some(o) = owner {
                for k in self.classes.get(&(fam, o)).into_iter().flatten() {
                    for b in &self.def(*k).bases {
                        let v = self.method(fam, b, name, false);
                        if !v.is_empty() {
                            return (v, Conf::Near);
                        }
                    }
                }
            }
            return (vec![], Conf::Near);
        }
        // 2. Foo.x() / Foo::x() / Foo.new: a known class
        if ident(recv) && self.classes.contains_key(&(fam, recv)) {
            let v = self.method(fam, recv, name, false);
            if !v.is_empty() {
                let mut v = v;
                v.sort_by_key(|g| g.0 != fi);
                return (v, Conf::Exact);
            }
            if CTORS.contains(&name) {
                if let Some(k) = self.class_named(fam, recv, fi, imported) {
                    return (vec![self.construct(k)], Conf::Exact);
                }
            }
            return (vec![], Conf::Exact); // Kind::Variant(..), Foo.CONSTANT(): nothing of the repo runs
        }
        // 3. a path written out: `crate::graph::link()`, `os.path.join()`, `fs::File::open()`
        if let Some(v) = self.qualified(fi, recv, name, imported) {
            return (v, Conf::Exact);
        }
        // 4. a name imported one by one: `from crud import make; make()`
        if recv.is_empty() && self.in_files(fam, name, &here, false, fi).is_empty() {
            let v = self.via_import(fi, name, 0);
            if !v.is_empty() {
                return (v, Conf::Exact);
            }
        }
        // `util.helper()`: a module or package the file imports by that name
        let module = !recv.is_empty() && ident(recv) && (f.aliases.iter().any(|a| a == recv)
            || imported.iter().any(|&i| crate::layers::stem(&self.files[i].path) == recv || crate::resolve::basename(dirname(&self.files[i].path)) == recv));
        if recv.is_empty() || module {
            // 5. the same file, 6. a file it imports (`helper()`, `util.helper()`), 7. the same package
            let mut scopes = vec![];
            if recv.is_empty() {
                scopes.push(&here);
            }
            scopes.push(imported);
            if recv.is_empty() {
                scopes.push(folder);
            }
            for scope in scopes {
                let v = self.in_files(fam, name, scope, false, fi);
                if !v.is_empty() {
                    return (one(v).into_iter().map(|g| self.construct(g)).collect(), Conf::Near);
                }
            }
            // 8. the only definition with that name in the repo, where names need no import (and it is not an imported library's)
            let imported_lib = f.aliases.iter().any(|a| a == name) && !f.bound.iter().any(|(a, _)| a == name);
            if recv.is_empty() && f.lang.scope == Scope::Repo && !imported_lib && name.len() >= 3 && !COMMON_NAMES.contains(&name) {
                let v: Vec<Gid> = self.top.get(&(fam, name)).into_iter().flatten().copied().filter(|g| self.live(*g, fi)).collect();
                if v.len() == 1 {
                    return (vec![self.construct(v[0])], Conf::Near);
                }
            }
            if recv.is_empty() {
                return (vec![], Conf::Near);
            }
        }
        if !recv.is_empty() && f.lang.scope == Scope::Repo && f.text[..c.at].ends_with("::") {
            // `module::func()` written out in full: the only top-level function of that name
            if name.len() >= 3 && !COMMON_NAMES.contains(&name) {
                let v: Vec<Gid> = self.top.get(&(fam, name)).into_iter().flatten().copied().filter(|g| self.def(*g).kind == Kind::Func && self.live(*g, fi)).collect();
                if v.len() == 1 {
                    return (v, Conf::Near);
                }
            }
        }
        // 9. x.y() on a variable made from a known class (`p = Parser()`, `const db = new Db()`)
        // (`self.svc.run()`, `this.repo.find()`: the declared type of that field)
        let field = recv.rsplit(['.', ':', '>']).next().unwrap_or(recv);
        let root = recv.split(['.', ':', '-']).next().unwrap_or(recv);
        let typed = types.get(recv).or_else(|| (field != recv && (SELF.contains(&root) || ident(root))).then(|| types.get(field)).flatten());
        if let Some(cls) = typed {
            let v = self.method(fam, cls, name, true);
            if !v.is_empty() {
                return (v, Conf::Near);
            }
        }
        // 10. x.y() on an object of unknown type: the method of that name, or "one of N" when several classes have it
        let word = receiver_word(recv).replace('_', "");
        if word.len() >= 3 {
            let singular = word.strip_suffix('s').unwrap_or(&word).to_string();
            let classes: Vec<&str> = [&word, &singular].iter().flat_map(|w| self.lower.get(&(fam, w.to_string())).into_iter().flatten().copied()).collect();
            for k in classes {
                let v = self.method(fam, k, name, true);
                if !v.is_empty() {
                    return (v, Conf::Guess);
                }
            }
        }
        if COMMON_METHODS.contains(&name) || name.len() < 3 {
            return (vec![], Conf::Guess);
        }
        // only classes the file can see through its imports: `tmp.path()` is not some other module's path()
        let seen = self.reaches(fi);
        let v: Vec<Gid> = self.any_method.get(&(fam, name)).into_iter().flatten().copied().filter(|g| seen.contains(&g.0) && self.live(*g, fi)).collect();
        if !(1..=6).contains(&v.len()) {
            return (vec![], Conf::Guess);
        }
        // and the classes built on them that replace that method
        let mut out: Vec<Gid> = v.iter().flat_map(|g| self.method(fam, self.def(*g).owner.as_deref().unwrap_or(""), name, true)).collect();
        out.extend(v);
        out.sort();
        out.dedup();
        out.truncate(6);
        (out, Conf::Guess)
    }
}

/// "src/graph.rs" -> "src/graph"; "app/__init__.py" -> "app"; "src/lib.rs" -> "src"
fn module_path(p: &str) -> String {
    let stem = p.rsplit_once('.').map(|x| x.0).unwrap_or(p);
    match stem.rsplit_once('/') {
        Some((d, last)) if ["index", "__init__", "mod", "lib", "main"].contains(&last) => d.to_string(),
        None if ["index", "__init__", "mod", "lib", "main"].contains(&stem) => String::new(),
        _ => stem.to_string(),
    }
}

/// What the SCIP index says about one call.
enum Verdict {
    /// the definitions it runs
    Defs(Vec<Gid>),
    /// defined outside the repo (or nowhere the index knows)
    Outside,
}

/// A SCIP index joined to the files of the repo: byte offsets of every line, for matching positions.
struct ScipView<'a> {
    scip: &'a crate::scip::Scip,
    lines: Vec<Vec<usize>>,
}

impl<'a> ScipView<'a> {
    fn new(scip: &'a crate::scip::Scip, files: &[File]) -> ScipView<'a> {
        let lines = files.iter().map(|f| std::iter::once(0).chain(f.text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1)).collect()).collect();
        ScipView { scip, lines }
    }

    fn line_text<'t>(&self, f: &'t File, fi: usize, line: usize) -> Option<(usize, &'t str)> {
        let ls = &self.lines[fi];
        let a = *ls.get(line)?;
        let b = ls.get(line + 1).copied().unwrap_or(f.text.len());
        Some((a, f.text.get(a..b)?.trim_end_matches(['\n', '\r'])))
    }

    /// the definition of ours that a definition of the index is
    fn def_gid(&self, ix: &Index, d: &crate::scip::DefLoc) -> Option<Gid> {
        let fi = *ix.by_path.get(self.scip.path_name(d.path))?;
        let f = &ix.files[fi];
        let (start, text) = self.line_text(f, fi, d.line as usize)?;
        let (a, b) = self.scip.name_bytes(d, text)?;
        let (a, b) = (start + a, start + b);
        let name = f.text.get(a..b)?;
        f.defs.iter().enumerate().skip(1)
            .filter(|(_, x)| matches!(x.kind, Kind::Func | Kind::Class | Kind::Type) && x.name == name && x.start <= a && a < x.end)
            .min_by_key(|(_, x)| x.end - x.start)
            .map(|(i, _)| (fi, i))
    }

    fn verdict(&self, ix: &Index, fi: usize, c: &Call) -> Option<Verdict> {
        let f = &ix.files[fi];
        // a call is at its callee name; a function handed over ends with its name
        let p = if c.is_ref { c.end.checked_sub(c.name.len())? } else { c.at };
        let line = self.lines[fi].partition_point(|&s| s <= p).checked_sub(1)?;
        let (start, text) = self.line_text(f, fi, line)?;
        let sym = self.scip.symbol_at(&f.path, line as u32, text, p - start)?;
        // a call on an interface can run any of its implementations
        let syms: Vec<u32> = std::iter::once(sym).chain(self.scip.implementors(sym).iter().copied().take(6)).collect();
        let mut out: Vec<Gid> = vec![];
        let mut unmapped = false;
        for s in syms {
            for d in self.scip.defs_of(s) {
                match self.def_gid(ix, d) {
                    Some(g) => {
                        let g = ix.construct(g);
                        if !out.contains(&g) {
                            out.push(g);
                        }
                    }
                    None => unmapped = true,
                }
            }
        }
        out.truncate(6);
        if !out.is_empty() {
            Some(Verdict::Defs(out))
        } else if unmapped {
            None // defined in the repo as something repoatlas does not model: its own answer stands
        } else {
            Some(Verdict::Outside)
        }
    }
}

/// `self.project`, `plugins[f["lang"]]` -> project, plugins
fn receiver_word(recv: &str) -> String {
    static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\[\]]*\]|\([^()]*\)").unwrap());
    let mut s = recv.to_string();
    for _ in 0..4 {
        let t = BRACKETS.replace_all(&s, "").to_string();
        if t == s {
            break;
        }
        s = t;
    }
    s.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).find(|w| !w.is_empty()).unwrap_or("").trim_start_matches('_').to_lowercase()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// `x = Foo(...)`, `const x = new Foo()`, `self.x = Foo()`, `x := Foo{}` -> {x: Foo}, for classes of the repo.
/// A name given two different classes says nothing.
fn var_types(f: &File, ix: &Index, fam: &'static str) -> HashMap<String, String> {
    static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)(?:^\s*|[;{(]\s*|\b(?:const|let|var|val|auto|my|local)\s+)((?:self\.|this\.|@|\$this->)?[A-Za-z_]\w*)\s*(?::\s*[\w.<>\[\]]+\s*)?:?=\s*(?:new\s+|&)?([A-Z]\w*)\s*[({]").unwrap()
    });
    let mut out: HashMap<String, String> = HashMap::new();
    let mut bad: HashSet<String> = HashSet::new();
    for c in ASSIGN.captures_iter(&f.code) {
        let (v, cls) = (c[1].to_string(), &c[2]);
        if !ix.classes.contains_key(&(fam, cls)) {
            continue;
        }
        if out.get(&v).is_some_and(|k| k != cls) {
            bad.insert(v.clone());
        }
        out.insert(v, cls.to_string());
    }
    // declared types: `p: Parser`, `svc: &Service`, `items: Optional[Item]`, `private svc: Service` (name: Type),
    // `Service svc;` `Parser p,` (Type name), `svc *Service` (Go)
    static NAME_TYPE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b([A-Za-z_]\w*)\s*\??:\s*(?:&\s*(?:'\w+\s+)?(?:mut\s+)?|\*|Optional\[|Annotated\[|Option<|Box<|Arc<|Rc<)?([A-Z]\w*)\b").unwrap()
    });
    static TYPE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z]\w*)(?:<[^<>;(){}]*>)?\s+([a-z_]\w*)\s*[;=,)]").unwrap());
    static GO_NAME_TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)\b([a-z_]\w*)\s+\*?([A-Z]\w*)\s*(?:[,)]|$)").unwrap());
    let mut declared: Vec<(String, String)> = NAME_TYPE.captures_iter(&f.code).map(|c| (c[1].to_string(), c[2].to_string())).collect();
    match f.lang.name {
        "Go" => declared.extend(GO_NAME_TYPE.captures_iter(&f.code).map(|c| (c[1].to_string(), c[2].to_string()))),
        "Java" | "C#" | "C++" | "Kotlin" | "Scala" | "PHP" => declared.extend(TYPE_NAME.captures_iter(&f.code).map(|c| (c[2].to_string(), c[1].to_string()))),
        _ => {}
    }
    for (v, cls) in declared {
        if !ix.classes.contains_key(&(fam, cls.as_str())) || bad.contains(&v) {
            continue;
        }
        if out.get(&v).is_some_and(|k| *k != cls) {
            bad.insert(v.clone());
            out.remove(&v);
        } else {
            out.insert(v, cls);
        }
    }
    for v in bad {
        out.remove(&v);
    }
    out
}

/// Functions a signature names through a module-level alias: `SessionDep = Annotated[Session, Depends(get_db)]`
/// at the top of a file, `def read(session: SessionDep)` anywhere. Calling `read` then runs `get_db` first; each
/// function comes back as a call placed in the signature: (file, call, what it runs).
fn alias_calls(files: &[File], per_file: &[Vec<(Vec<Gid>, Conf)>]) -> Vec<(usize, Call, Vec<Gid>)> {
    static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^(?:export\s+)?(?:(?:const|let|var)\s+)?([A-Za-z_]\w*)\s*(?::[^=\n]+)?=[^\n]*").unwrap()
    });
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z_]\w*").unwrap());
    let mut aliases: HashMap<(usize, String), Vec<Gid>> = HashMap::new();
    for (fi, f) in files.iter().enumerate() {
        for c in ASSIGN.captures_iter(&f.bare) {
            let whole = c.get(0).unwrap();
            let gids: Vec<Gid> = f.calls.iter().enumerate()
                .filter(|(_, k)| k.is_ref && k.def == 0 && k.at >= whole.start() && k.at < whole.end())
                .flat_map(|(ci, _)| per_file[fi][ci].0.iter().copied())
                .filter(|g| files[g.0].defs[g.1].kind == Kind::Func)
                .collect();
            if !gids.is_empty() {
                aliases.insert((fi, c[1].to_string()), gids);
            }
        }
    }
    if aliases.is_empty() {
        return vec![];
    }
    let by_path: HashMap<&str, usize> = files.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
    let mut out = vec![];
    for (fi, f) in files.iter().enumerate() {
        for (di, d) in f.defs.iter().enumerate().skip(1).filter(|(_, d)| d.kind == Kind::Func) {
            // the part of the definition before its body: up to `):`, `) ->` or `{`
            let head = crate::text::slice(&f.code, d.start, d.end.min(d.start + 800));
            let cut = ["):", ") ->", ") {", "{"].iter().filter_map(|m| head.find(m).map(|i| i + m.len())).min().unwrap_or(head.len().min(300));
            let head = &head[..cut.min(head.len())];
            let mut seen = HashSet::new();
            for w in WORD.find_iter(head) {
                let name = w.as_str();
                if name == d.name || !seen.insert(name) {
                    continue;
                }
                let key = if aliases.contains_key(&(fi, name.to_string())) {
                    Some((fi, name.to_string()))
                } else {
                    f.bound_names.iter().find(|(l, ..)| l == name).and_then(|(_, r, p)| by_path.get(p.as_str()).map(|&j| (j, r.clone())))
                };
                let Some(gs) = key.and_then(|k| aliases.get(&k)) else { continue };
                let at = d.start + w.start();
                for g in gs {
                    let call = Call {
                        name: files[g.0].defs[g.1].name.clone(), recv: String::new(), at, end: d.start + w.end(), from: at,
                        line: crate::text::line_of(&f.text, at), def: di, is_ref: true, cond: "",
                    };
                    out.push((fi, call, vec![*g]));
                }
            }
        }
    }
    out
}

/// For each file, for each of its calls: the definitions it runs (empty = outside the repo), and whether that is
/// only a guess read off a name. `libs`: crate name -> its lib.rs. Calls a signature hides behind an alias are added
/// to the files (kept in source order).
pub fn resolve_calls(files: &mut [File], libs: &HashMap<String, String>, scip: Option<&crate::scip::Scip>) -> (Vec<Vec<Vec<Gid>>>, Vec<Vec<bool>>, usize) {
    let hits = std::cell::Cell::new(0usize);
    let mut per_file: Vec<Vec<(Vec<Gid>, Conf)>> = {
        let files: &[File] = files;
        let ix = Index::new(files, libs);
        let view = scip.map(|s| ScipView::new(s, files));
        let mut folders: HashMap<(&str, &str), HashSet<usize>> = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            folders.entry((family(f.lang), dirname(&f.path))).or_default().insert(i);
        }
        files
            .iter()
            .enumerate()
            .map(|(fi, f)| {
                let fam = family(f.lang);
                let mut imported: HashSet<usize> = f.uses.iter().filter_map(|u| ix.by_path.get(u.as_str()).copied()).collect();
                if f.lang.scope == Scope::Folder {
                    // an imported package is every file of its folder
                    let more: Vec<usize> = imported.iter().flat_map(|&i| folders.get(&(fam, dirname(&files[i].path))).into_iter().flatten().copied()).collect();
                    imported.extend(more);
                }
                imported.remove(&fi);
                let mut folder = if f.lang.scope != Scope::Imports { folders.get(&(fam, dirname(&f.path))).cloned().unwrap_or_default() } else { HashSet::new() };
                folder.remove(&fi);
                let types = var_types(f, &ix, fam);
                f.calls.iter().map(|c| {
                    let ours = ix.resolve(fi, c, &imported, &folder, &types);
                    // the index knows better: what it names is used, and an outside library beats a guess from a name
                    match view.as_ref().and_then(|v| v.verdict(&ix, fi, c)) {
                        Some(Verdict::Defs(t)) => {
                            hits.set(hits.get() + 1);
                            (t, Conf::Exact)
                        }
                        Some(Verdict::Outside) if ours.1 == Conf::Guess || ours.0.is_empty() => {
                            hits.set(hits.get() + 1);
                            (vec![], Conf::Exact)
                        }
                        _ => ours,
                    }
                }).collect()
            })
            .collect()
    };
    let mut extra: Vec<(usize, Call, Vec<Gid>)> = alias_calls(files, &per_file);
    extra.sort_by_key(|(fi, c, _)| (*fi, c.at));
    for (fi, call, targets) in extra {
        let f = &mut files[fi];
        // keep the calls in source order: the signature comes before the body
        let at = f.calls.partition_point(|c| c.at <= call.at);
        f.calls.insert(at, call);
        per_file[fi].insert(at, (targets, Conf::Exact));
    }
    let guess = per_file.iter().map(|v| v.iter().map(|(_, c)| *c == Conf::Guess).collect()).collect();
    let targets = per_file.into_iter().map(|v| v.into_iter().map(|(t, _)| t).collect()).collect();
    (targets, guess, hits.get())
}

/// What each definition's own body takes in and gives out (nested definitions count for themselves).
/// carried: names bound to a module of the repo that opens a database / HTTP / queue client, with what it opens.
pub fn def_io(f: &mut File, carried: &[(String, &'static str)], orm: &crate::orm::Index) {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z_]\w*").unwrap());
    let n = f.defs.len();
    let mut kids: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, d) in f.defs.iter().enumerate() {
        if let Some(p) = d.parent {
            kids[p].push(i);
        }
    }
    // outside libraries that carry I/O (a database driver, an HTTP client): the definitions that name them
    let mut libs: Vec<(String, &'static str)> = vec![];
    for (name, spec) in &f.ext {
        if let Some(io) = cat_of(spec).and_then(io_of_cat) {
            let local = spec.rsplit(['/', '.']).next().unwrap_or(name).replace('-', "_");
            libs.push((local, io));
        }
    }
    // names imported from such a library: `from sqlmodel import Session`, `import axios from 'axios'`
    for imp in f.py_imports.iter().filter(|i| i.is_from) {
        if let Some(io) = cat_of(&imp.module).and_then(io_of_cat) {
            libs.extend(imp.names.iter().filter(|n| *n != "*").map(|n| (n.clone(), io)));
        }
    }
    for (alias, spec) in &f.bindings {
        if let Some(io) = cat_of(spec).and_then(io_of_cat) {
            libs.push((alias.clone(), io));
        }
    }
    libs.extend(carried.iter().cloned());
    let mut lib_found: HashSet<&'static str> = HashSet::new();
    let mut ios = vec![vec![]; n];
    let mut dbs: Vec<Vec<String>> = vec![vec![]; n];
    let sessions = crate::orm::sessions(f, orm);
    for i in 0..n {
        let d = &f.defs[i];
        if matches!(d.kind, Kind::Class | Kind::Type | Kind::Impl) && i != 0 {
            continue;
        }
        let nested: Vec<(usize, usize)> = kids[i].iter().map(|&k| &f.defs[k]).filter(|kd| matches!(kd.kind, Kind::Func | Kind::Main)).map(|kd| (kd.start, kd.end)).collect();
        let code = own_text(&f.code, d.start, d.end, &nested);
        let bare = own_text(&f.bare, d.start, d.end, &nested);
        let mut found: Vec<&'static str> = IO_RES
            .iter()
            .filter(|(k, re)| re.is_match(if IN_STRINGS.contains(k) { &bare } else { &code }))
            .map(|(k, _)| *k)
            .collect();
        if !libs.is_empty() {
            let words: HashSet<&str> = WORD.find_iter(&code).map(|m| m.as_str()).collect();
            for (local, io) in &libs {
                if i != 0 && words.contains(local.as_str()) {
                    found.push(io);
                    lib_found.insert(io);
                }
            }
        }
        if d.route.is_some() || f.routes.iter().any(|r| r.def == i) {
            found.push("http_in");
        }
        // an ORM call says more than the library it comes from: "reads User" instead of just "database"
        let labels = crate::orm::accesses(&code, f.lang.name, &sessions, &orm.models);
        if !labels.is_empty() {
            found.retain(|k| *k != "db");
            if labels.iter().any(|l| l.starts_with("reads")) {
                found.push("db_read");
            }
            if labels.iter().any(|l| l.starts_with("writes")) {
                found.push("db_write");
            }
        }
        dbs[i] = labels;
        ios[i] = found;
    }
    for (_, io) in &libs {
        if !lib_found.contains(io) {
            ios[0].push(io); // named only at the top of the file
        }
    }
    let has_routes = f.symbols.iter().any(|s| s.k == "route");
    let mut all: BTreeSet<&'static str> = BTreeSet::new();
    for (i, mut io) in ios.into_iter().enumerate() {
        if io.contains(&"http_in") {
            io.push("http_resp");
        }
        io.sort_by_key(|k| io_order(k));
        io.dedup();
        all.extend(io.iter().copied());
        f.defs[i].io = io;
        f.defs[i].db = std::mem::take(&mut dbs[i]);
    }
    if has_routes {
        all.insert("http_in");
        all.insert("http_resp");
    }
    let mut all: Vec<&'static str> = all.into_iter().collect();
    all.sort_by_key(|k| io_order(k));
    f.io = all;
}

/// text[a..b] without the nested ranges (each becomes a line break, so no two words join across one)
fn own_text(text: &str, a: usize, b: usize, nested: &[(usize, usize)]) -> String {
    let mut out = String::new();
    let mut at = a;
    let mut nested = nested.to_vec();
    nested.sort();
    for &(s, e) in &nested {
        if s >= at {
            out.push_str(crate::text::slice(text, at, s));
            out.push('\n');
            at = e;
        }
    }
    out.push_str(crate::text::slice(text, at, b));
    out
}
