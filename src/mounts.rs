//! Web route prefixes. A route is registered on an object (`router.get("/")`), that object is mounted on
//! another with a prefix (`app.include_router(router, prefix="/api")`, `app.use("/api", routes)`,
//! `v1 := r.Group("/v1")`), and the full path is every prefix on the way up, joined.
//! The prefix may be a constant defined elsewhere (`settings.API_V1_STR`): it is looked up, and left as `{NAME}`
//! when the repo does not give one value for it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::parse::File;
use crate::rules::{mount_call, routes_file, GROUP_CALLS, INCLUDE, PATTERN_PARTS, ROOT_LISTS, SCOPE_ARRAY, SCOPE_CALLS, SCOPE_CHAIN, SCOPE_FIRST_ARG};
use crate::tables::STEM;

/// (file, name of the router object in it); the name `*` stands for every router a file builds
type Key = (usize, String);

struct Edge {
    parent: Key,
    child: Key,
    path: String,
}

/// routes written inside a closure that carries a prefix: (file, start, end, prefix)
struct Scope {
    file: usize,
    from: usize,
    to: usize,
    path: String,
}

#[derive(Default)]
pub struct Mounts {
    /// prefix a router carries itself: `APIRouter(prefix="/items")`
    own: HashMap<Key, String>,
    edges: Vec<Edge>,
    scopes: Vec<Scope>,
    /// per file: the lists of routes it writes, (start, end, name), the name of the list that is the file itself being ""
    lists: HashMap<usize, Vec<(usize, usize, String)>>,
}

/// The arguments of the call that `text` starts with (`include_router(a, prefix="/x")`), split at top-level commas.
pub(crate) fn args_of(text: &str) -> Vec<String> {
    let Some(open) = text.find('(') else { return vec![] };
    if text[..open].contains(['\n', ';', '{', '=', ' ']) {
        return vec![];
    }
    let (mut out, mut cur, mut depth, mut quote) = (vec![], String::new(), 0i32, None::<char>);
    for ch in text[open + 1..].chars() {
        if let Some(q) = quote {
            cur.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => {
                quote = Some(ch);
                cur.push(ch);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(ch);
            }
            ')' | ']' | '}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// `prefix="/x"` -> ("prefix", "\"/x\"")
pub(crate) fn kwarg(a: &str) -> Option<(&str, &str)> {
    static KW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z_]\w*)\s*[=:]\s*(.+)$").unwrap());
    let c = KW.captures(a)?;
    let v = c.get(2)?;
    (!v.as_str().starts_with('=')).then(|| (c.get(1).unwrap().as_str(), v.as_str()))
}

fn is_literal(a: &str) -> bool {
    let t = a.trim_start_matches(|c: char| "rRbBuUfF".contains(c));
    t.starts_with(['"', '\'', '`'])
}

/// The arguments, with an options object opened up into its entries where the language passes options that way:
/// `{ prefix: '/api' }` -> `prefix: '/api'`
fn open_options(lang: &str, args: Vec<String>) -> Vec<String> {
    if !crate::rules::OPTION_OBJECT_LANGS.contains(&lang) {
        return args;
    }
    args.into_iter().flat_map(|a| match a.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
        Some(inner) => args_of(&format!("({inner})")),
        None => vec![a],
    }).collect()
}

/// Prefix arguments of a mount or router call: (path written first, `prefix=` / `url_prefix=` value, the other positionals)
fn split_prefix(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut path = None;
    let mut rest = vec![];
    for a in args {
        match kwarg(a) {
            Some(("prefix" | "url_prefix" | "path", v)) => path = Some(v.to_string()),
            Some(_) => {}
            None => rest.push(a.clone()),
        }
    }
    if path.is_none() && rest.first().is_some_and(|a| is_literal(a)) {
        path = Some(rest.remove(0));
    }
    (path, rest)
}

struct Eval<'a> {
    files: &'a [File],
    consts: RefCell<HashMap<String, Option<String>>>,
}

impl Eval<'_> {
    /// The one string a constant is given anywhere in the repo.
    fn constant(&self, name: &str) -> Option<String> {
        if let Some(v) = self.consts.borrow().get(name) {
            return v.clone();
        }
        let re = Regex::new(&format!(r#"(?m)\b{}\b\s*(?::[^=\n]+)?=\s*[rbuf]*["']([^"'\n]*)["']"#, regex::escape(name))).unwrap();
        let mut vals: Vec<String> = self.files.iter().flat_map(|f| re.captures_iter(&f.bare).map(|c| c[1].to_string()).collect::<Vec<_>>()).collect();
        vals.sort();
        vals.dedup();
        let v = (vals.len() == 1).then(|| vals.remove(0));
        self.consts.borrow_mut().insert(name.to_string(), v.clone());
        v
    }

    /// `"/api"`, `f"{settings.API}/v1"`, `settings.API_V1_STR`, `BASE + "/x"` -> "/api", "/api/v1", "/api/v1", ...
    fn eval(&self, expr: &str) -> String {
        static HOLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$?\{\s*([\w.]+)\s*\}").unwrap());
        let expr = expr.trim();
        if expr.contains(" + ") {
            return expr.split(" + ").map(|p| self.eval(p)).collect();
        }
        if is_literal(expr) {
            let inner = expr.trim_start_matches(|c: char| "rRbBuUfF".contains(c)).trim_matches(['"', '\'', '`']);
            return HOLE.replace_all(inner, |c: &regex::Captures| self.name(&c[1])).into_owned();
        }
        if expr.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$') && !expr.is_empty() {
            return self.name(expr);
        }
        String::new()
    }

    fn name(&self, dotted: &str) -> String {
        let last = dotted.rsplit('.').next().unwrap_or(dotted);
        self.constant(last).unwrap_or_else(|| format!("{{{last}}}"))
    }
}

/// Where a router object named in an argument lives: `items.router`, `api_router`, `routes` (a module imported whole).
fn child_key(f: &File, fi: usize, by_path: &HashMap<&str, usize>, expr: &str) -> Option<Key> {
    static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$").unwrap());
    if !PATH.is_match(expr) {
        return None;
    }
    let segs: Vec<&str> = expr.split('.').collect();
    let file_of = |p: &str| by_path.get(p).copied();
    if segs.len() >= 2 {
        let i = f.bound.iter().find(|(a, _)| a == segs[0]).and_then(|(_, p)| file_of(p))?;
        return Some((i, segs[segs.len() - 1].to_string()));
    }
    let x = segs[0];
    if let Some((_, remote, p)) = f.bound_names.iter().find(|(l, ..)| l == x) {
        if let Some(i) = file_of(p) {
            return Some((i, remote.clone()));
        }
    }
    if let Some(i) = f.bound.iter().find(|(a, _)| a == x).and_then(|(_, p)| file_of(p)) {
        return Some((i, "*".to_string()));
    }
    Some((fi, x.to_string()))
}

/// The router a mount call is made on: `app` in `app.use(..)`, and for a chain (`const api = Router().use(a).use(b)`,
/// `export default Router().use("/api", api)`) the name the whole chain is given.
fn mount_parent(f: &File, c: &crate::parse::Call) -> Option<String> {
    static TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\w+)\s*(?::[^=\n]+)?=\s*$").unwrap());
    static PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$").unwrap());
    if PLAIN.is_match(&c.recv) {
        return Some(c.recv.rsplit('.').next().unwrap_or(&c.recv).to_string());
    }
    let root = c.recv.split('(').next().unwrap_or("").trim();
    if let Some((obj, _)) = root.rsplit_once('.') {
        // app.use(a).use(b): the object the first call was made on
        return PLAIN.is_match(obj).then(|| obj.rsplit('.').next().unwrap_or(obj).to_string());
    }
    let line = f.bare[..c.from.min(f.bare.len())].rfind('\n').map_or(0, |i| i + 1);
    let head = &f.bare[line..c.from.min(f.bare.len())];
    if let Some(t) = TARGET.captures(head) {
        return Some(t[1].to_string());
    }
    head.trim_end().ends_with("export default").then(|| "default".to_string())
}

/// Where the bracket opened at `open` closes: the index after it, or the end of the text when it never does. Quoted text is skipped.
fn close_of(text: &str, open: usize) -> usize {
    let (mut depth, mut quote) = (0i32, None::<char>);
    let mut chars = text[open..].char_indices();
    while let Some((i, ch)) = chars.next() {
        if let Some(q) = quote {
            if ch == '\\' {
                chars.next();
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return open + i + 1;
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// A route pattern as the path people see: `^users/(?P<pk>\d+)/orders/?` -> users/{pk}/orders
fn pattern_path(pattern: &str) -> String {
    PATTERN_PARTS.iter().fold(pattern.to_string(), |p, (re, to)| re.replace_all(&p, *to).into_owned())
}

/// The one file a dotted module name (`shop.urls`) is, wherever the project root is.
fn module_file(files: &[File], dotted: &str) -> Option<usize> {
    let stem = dotted.replace('.', "/");
    let mut hits = files.iter().enumerate().filter(|(_, f)| {
        let p = STEM.replace(&f.path, "");
        p == stem || p.ends_with(&format!("/{stem}"))
    });
    match (hits.next(), hits.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

pub fn build(files: &[File]) -> Mounts {
    static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^[ \t]*(?:(?:export\s+)?(?:const|let|var|val)\s+)?([A-Za-z_]\w*)\s*(?::[^=\n]+)?:?=\s*(new\s+)?([A-Za-z_][\w.]*)\(").unwrap()
    });
    let by_path: HashMap<&str, usize> = files.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
    let ev = Eval { files, consts: RefCell::new(HashMap::new()) };
    let mut m = Mounts::default();
    // the objects routes are registered on: every name in `api.router.add_get(..)` may be the one a mount names
    let routed: HashSet<Key> = files.iter().enumerate()
        .flat_map(|(fi, f)| f.routes.iter().map(|r| r.recv.as_str()).chain(f.defs.iter().map(|d| d.route_on.as_str())).map(move |recv| (fi, recv)))
        .flat_map(|(fi, recv)| recv.split('.').filter(|n| !n.is_empty()).map(move |n| (fi, n.to_string())))
        .collect();
    for (fi, f) in files.iter().enumerate() {
        if f.lang.grammar.is_none() {
            continue;
        }
        let options = crate::rules::OPTION_OBJECT_LANGS.contains(&f.lang.name);
        // a router built with a prefix of its own, or a sub-router made by a group call
        for c in ASSIGN.captures_iter(&f.bare) {
            // `new Router({ prefix: '/api' })` where options come as an object
            if c.get(2).is_some() && !options {
                continue;
            }
            let (name, callee) = (c[1].to_string(), c.get(3).unwrap());
            let args = open_options(f.lang.name, args_of(&f.bare[callee.start()..]));
            let (path, _) = split_prefix(&args);
            let Some(path) = path else { continue };
            let last = callee.as_str().rsplit('.').next().unwrap_or("");
            if GROUP_CALLS.contains(&last) {
                if let Some((parent, _)) = callee.as_str().rsplit_once('.') {
                    let parent = parent.rsplit('.').next().unwrap_or(parent);
                    m.edges.push(Edge { parent: (fi, parent.to_string()), child: (fi, name), path: ev.eval(&path) });
                }
            } else if args.iter().any(|a| matches!(kwarg(a), Some(("prefix" | "url_prefix", _)))) {
                m.own.insert((fi, name), ev.eval(&path));
            }
        }
        // one router mounted on another: by the calls that are known to, or any call that hands a router (an object that has
        // routes registered on it) over under a path: `app.add_subapp("/api", api)`
        let mounts_one = |c: &crate::parse::Call| -> bool {
            if mount_call(f.lang.name, &c.name) {
                return true;
            }
            let args = args_of(&f.bare[c.at..c.end.min(f.bare.len())]);
            let (path, rest) = split_prefix(&args);
            // the prefix is a URL path written out (not `":80"`, the address `ListenAndServe(":80", r)` serves on)
            path.is_some_and(|p| is_literal(&p) && p.trim_start_matches(|c: char| "rRbBuUfF".contains(c)).trim_start_matches(['"', '\'', '`']).starts_with('/')) && rest.len() == 1 && child_key(f, fi, &by_path, &rest[0]).is_some_and(|k| routed.contains(&k))
        };
        for c in f.calls.iter().filter(|c| !c.is_ref && !c.recv.is_empty() && mounts_one(c)) {
            let args = open_options(f.lang.name, args_of(&f.bare[c.at..c.end.min(f.bare.len())]));
            let (path, rest) = split_prefix(&args);
            let Some(child) = rest.first().and_then(|a| child_key(f, fi, &by_path, a)) else { continue };
            let Some(parent) = mount_parent(f, c) else { continue };
            let parent = (fi, parent);
            if parent != child {
                m.edges.push(Edge { parent, child, path: path.map(|p| ev.eval(&p)).unwrap_or_default() });
            }
        }
        // a routes file, or a list of routes, handed to another under a prefix: `path("api/", include("shop.urls"))`,
        // `include((v1_patterns, "v1"))`, `include([path(..)])`
        if let Some(calls) = routes_file(&f.path) {
            static LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*([A-Za-z_]\w*)\s*(?::[^=\n]+)?\+?=\s*\[").unwrap());
            let mut lists: Vec<(usize, usize, String)> = LIST.captures_iter(&f.bare).map(|c| {
                let open = c.get(0).unwrap().end() - 1;
                let name = if ROOT_LISTS.contains(&&c[1]) { String::new() } else { c[1].to_string() };
                (open, close_of(&f.bare, open), name)
            }).collect();
            // what each route call hands over: (the call, the pattern, what it includes)
            let mut includes: Vec<(usize, String, String)> = vec![];
            for c in f.calls.iter().filter(|c| !c.is_ref && calls.contains(&c.name.as_str())) {
                let end = c.end.min(f.bare.len());
                let args = args_of(&f.bare[c.at..end]);
                let (Some(pattern), Some(handler)) = (args.first(), args.get(1)) else { continue };
                if !INCLUDE.is_match(handler) {
                    continue;
                }
                let Some(first) = args_of(handler).into_iter().next() else { continue };
                // `include((target, "namespace"))`: the target is the first of the pair
                let target = if first.starts_with('(') { args_of(&format!("t{first}")).into_iter().next().unwrap_or_default() } else { first };
                if target.starts_with('[') {
                    // a list written in place: a router of its own, under this call
                    if let Some(open) = f.bare[c.at..end].find("include").and_then(|i| f.bare[c.at + i..end].find('[').map(|j| c.at + i + j)) {
                        lists.push((open, close_of(&f.bare, open), format!("@{open}")));
                        includes.push((c.at, pattern.clone(), format!("@{open}")));
                    }
                } else {
                    includes.push((c.at, pattern.clone(), target));
                }
            }
            m.lists.insert(fi, lists);
            for (at, pattern, target) in includes {
                let child = if let Some(local) = target.strip_prefix('@') {
                    Some((fi, format!("@{local}")))
                } else if is_literal(&target) {
                    module_file(files, target.trim_start_matches(|c: char| "rRbBuUfF".contains(c)).trim_matches(['"', '\''])).map(|i| (i, String::new()))
                } else {
                    // a list by name, written here or imported from another file
                    child_key(f, fi, &by_path, &target).map(|(i, name)| (i, if ROOT_LISTS.contains(&name.as_str()) { String::new() } else { name }))
                };
                let Some(child) = child else { continue };
                let path = pattern_path(ev.eval(&pattern).trim_start_matches('^'));
                m.edges.push(Edge { parent: (fi, m.list_at(fi, at)), child, path });
            }
        }
        // routes written inside a closure that carries a prefix: `r.Route("/v1", func(r) {..})`, `Route::prefix('api')->group(function () {..})`
        static CLOSURE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bfunction\b|\bfunc\b|\bfn\b|=>").unwrap());
        for c in f.calls.iter().filter(|c| !c.is_ref && SCOPE_CALLS.contains(&c.name.as_str())) {
            let end = c.end.min(f.bare.len());
            let text = &f.bare[c.at..end];
            let Some(open) = text.find('(') else { continue };
            let args = &text[open..];
            if !CLOSURE.is_match(args) {
                continue;
            }
            let prefix = SCOPE_FIRST_ARG.captures(args).or_else(|| SCOPE_CHAIN.captures(&c.recv)).or_else(|| SCOPE_ARRAY.captures(args));
            if let Some(p) = prefix {
                m.scopes.push(Scope { file: fi, from: c.from, to: end, path: ev.eval(&format!("\"{}\"", &p[1])) });
            }
        }
        // routers nested in a chain that has a path: `Router::with_path("api").push(Router::with_path("users").get(list))`
        if let Some(ch) = crate::rules::chain_of(f.lang.name) {
            for c in f.calls.iter().filter(|c| !c.is_ref && ch.nests.contains(&c.name.as_str())) {
                if let Some(p) = crate::parse::chain_path(f.lang.name, &c.recv) {
                    m.scopes.push(Scope { file: fi, from: c.at, to: c.end.min(f.bare.len()), path: ev.eval(&format!("\"{p}\"")) });
                }
            }
        }
    }
    m
}

/// "/api" + "/items" + "/" -> "/api/items/"; "x" -> "/x"
pub fn join_path(parts: &[&str]) -> String {
    let kept: Vec<&str> = parts.iter().map(|p| p.trim_matches('/')).filter(|p| !p.is_empty()).collect();
    let mut out = format!("/{}", kept.join("/"));
    if parts.last().is_some_and(|p| p.ends_with('/')) && !out.ends_with('/') {
        out.push('/');
    }
    out
}

impl Mounts {
    /// every prefix the router `key` is reached under (a router mounted in several places answers under each)
    fn totals(&self, key: &Key, depth: usize) -> Vec<String> {
        let own = self.own.get(key).cloned().unwrap_or_default();
        if depth > 5 {
            return vec![own];
        }
        let mut edges: Vec<&Edge> = self.edges.iter().filter(|e| e.child == *key).collect();
        if edges.is_empty() {
            edges.extend(self.edges.iter().find(|e| !key.1.is_empty() && e.child.0 == key.0 && e.child.1 == "*" && e.parent != *key));
        }
        if edges.is_empty() {
            return vec![own];
        }
        let mut out: Vec<String> = vec![];
        for e in edges {
            for up in self.totals(&e.parent, depth + 1) {
                let p = join_path(&[&up, &e.path, &own]);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out.truncate(6);
        out
    }

    /// the list of routes the byte `at` of file `fi` is written in ("" for the file itself)
    fn list_at(&self, fi: usize, at: usize) -> String {
        let lists = self.lists.get(&fi).map(|v| v.as_slice()).unwrap_or(&[]);
        lists.iter().filter(|(a, b, _)| *a <= at && at < *b).min_by_key(|(a, b, _)| b - a).map(|l| l.2.clone()).unwrap_or_default()
    }

    /// The prefixes of every route registered on `recv` in file `fi` at byte `at`: those of the router it is mounted on, then those
    /// of the closures it is written in. One prefix per place the router is mounted; never empty ("" when it is mounted nowhere).
    pub fn prefixes(&self, fi: usize, at: usize, recv: &str) -> Vec<String> {
        // a route with no object (`path("x/", view)`) is on the list of routes it is written in
        // (on `api.router` the name that is mounted is the one with a mount: the last name that has one)
        let mounted = |n: &str| self.own.contains_key(&(fi, n.to_string())) || self.edges.iter().any(|e| e.child == (fi, n.to_string()));
        let name = if recv.is_empty() {
            self.list_at(fi, at)
        } else {
            recv.rsplit('.').find(|n| mounted(n)).unwrap_or_else(|| recv.rsplit('.').next().unwrap_or(recv)).to_string()
        };
        let mut inner: Vec<&Scope> = self.scopes.iter().filter(|s| s.file == fi && s.from <= at && at < s.to).collect();
        inner.sort_by_key(|s| s.from);
        let mut out: Vec<String> = vec![];
        for up in self.totals(&(fi, name), 0) {
            let mut parts: Vec<&str> = vec![&up];
            parts.extend(inner.iter().map(|s| s.path.as_str()));
            let p = join_path(&parts);
            let p = if p == "/" { String::new() } else { p };
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }
}
