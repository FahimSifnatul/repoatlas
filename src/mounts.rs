//! Web route prefixes. A route is registered on an object (`router.get("/")`), that object is mounted on
//! another with a prefix (`app.include_router(router, prefix="/api")`, `app.use("/api", routes)`,
//! `v1 := r.Group("/v1")`), and the full path is every prefix on the way up, joined.
//! The prefix may be a constant defined elsewhere (`settings.API_V1_STR`): it is looked up, and left as `{NAME}`
//! when the repo does not give one value for it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::parse::File;

/// calls that put one router under another
const MOUNT_CALLS: &[&str] = &["include_router", "register_blueprint", "use", "mount", "Mount", "add_router"];
/// calls that make a sub-router with a prefix: `v1 := r.Group("/v1")`
const GROUP_CALLS: &[&str] = &["Group", "group", "Party"];

/// (file, name of the router object in it); the name `*` stands for every router a file builds
type Key = (usize, String);

struct Edge {
    parent: Key,
    child: Key,
    path: String,
}

#[derive(Default)]
pub struct Mounts {
    /// prefix a router carries itself: `APIRouter(prefix="/items")`
    own: HashMap<Key, String>,
    edges: Vec<Edge>,
}

/// The arguments of the call that `text` starts with (`include_router(a, prefix="/x")`), split at top-level commas.
fn args_of(text: &str) -> Vec<String> {
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
fn kwarg(a: &str) -> Option<(&str, &str)> {
    static KW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z_]\w*)\s*[=:]\s*(.+)$").unwrap());
    let c = KW.captures(a)?;
    let v = c.get(2)?;
    (!v.as_str().starts_with('=')).then(|| (c.get(1).unwrap().as_str(), v.as_str()))
}

fn is_literal(a: &str) -> bool {
    let t = a.trim_start_matches(|c: char| "rRbBuUfF".contains(c));
    t.starts_with(['"', '\'', '`'])
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

pub fn build(files: &[File]) -> Mounts {
    static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^[ \t]*(?:(?:export\s+)?(?:const|let|var|val)\s+)?([A-Za-z_]\w*)\s*(?::[^=\n]+)?:?=\s*([A-Za-z_][\w.]*)\(").unwrap()
    });
    let by_path: HashMap<&str, usize> = files.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
    let ev = Eval { files, consts: RefCell::new(HashMap::new()) };
    let mut m = Mounts::default();
    for (fi, f) in files.iter().enumerate() {
        if f.lang.grammar.is_none() {
            continue;
        }
        // a router built with a prefix of its own, or a sub-router made by a group call
        for c in ASSIGN.captures_iter(&f.bare) {
            let (name, callee) = (c[1].to_string(), c.get(2).unwrap());
            let args = args_of(&f.bare[callee.start()..]);
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
        // one router mounted on another
        for c in f.calls.iter().filter(|c| !c.is_ref && MOUNT_CALLS.contains(&c.name.as_str()) && !c.recv.is_empty()) {
            let args = args_of(&f.bare[c.at..c.end.min(f.bare.len())]);
            let (path, rest) = split_prefix(&args);
            let Some(child) = rest.first().and_then(|a| child_key(f, fi, &by_path, a)) else { continue };
            let Some(parent) = mount_parent(f, c) else { continue };
            let parent = (fi, parent);
            if parent != child {
                m.edges.push(Edge { parent, child, path: path.map(|p| ev.eval(&p)).unwrap_or_default() });
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
    fn total(&self, key: &Key, depth: usize) -> String {
        let own = self.own.get(key).cloned().unwrap_or_default();
        if depth > 5 {
            return own;
        }
        let edge = self.edges.iter().find(|e| e.child == *key)
            .or_else(|| self.edges.iter().find(|e| e.child.0 == key.0 && e.child.1 == "*" && e.parent != *key));
        match edge {
            Some(e) => {
                let up = self.total(&e.parent, depth + 1);
                join_path(&[&up, &e.path, &own])
            }
            None => own,
        }
    }

    /// The prefix of every route registered on `recv` in file `fi` ("" when it is not mounted anywhere).
    pub fn prefix(&self, fi: usize, recv: &str) -> String {
        let var = recv.rsplit('.').next().unwrap_or(recv);
        if var.is_empty() {
            return String::new();
        }
        let p = self.total(&(fi, var.to_string()), 0);
        if p == "/" { String::new() } else { p }
    }
}
