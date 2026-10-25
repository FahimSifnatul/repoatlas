//! One file -> what it defines, what it calls, its imports and routes, from the tree-sitter syntax tree.
//! Everything here is driven by the language's query (queries/*.scm); nothing names a language's syntax.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, QueryCursor};

use crate::lang::{self, Lang, Resolver};
use crate::rules::{routes_file, BARE_VERB_DECORATOR, BARE_VERB_DECORATOR_LANGS, NAMED_ROUTE_ARGS, NAMED_ROUTE_CALLS, ROUTE_TABLES, ROUTE_TABLE_ENTRY, VIEW_ROUTE_NAME, VIEW_VERB, PATTERN_PARTS, CLASS_PREFIX, DECORATOR_ROUTES, HANDLER_FORMS, HANDLER_KWARGS, HANDLER_WRAP, INCLUDE, METHODS_AFTER, RESOURCES, ROUTE_BARE, ROUTE_CALLS, ROUTE_RES};
use crate::tables::{COMMENTS, STRINGS};
use crate::text::{first_sentence, header_doc, lead_comment, line_of, slice, squash_ws};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Class,
    Type,
    /// `impl Type`: holds methods of Type, is not a symbol of its own
    Impl,
    Func,
    /// the top level of a file: code that runs when the file is loaded or run
    Module,
    /// `if __name__ == "__main__":` and the like
    Main,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: String,
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub parent: Option<usize>,
    /// the class a method belongs to
    pub owner: Option<String>,
    pub bases: Vec<String>,
    pub sig: String,
    pub doc: String,
    pub route: Option<(String, String)>,
    /// the object a route is registered on: `router` in `@router.get("/x")`
    pub route_on: String,
    /// a class's own route prefix: `@RequestMapping("/users")`, `@Controller('users')`
    pub prefix: String,
    /// a view that answers a route registered elsewhere by name: (route name, verb or "ANY")
    pub view_for: Option<(String, String)>,
    /// what this definition's own body takes in and gives out (filled by analyze)
    pub io: Vec<&'static str>,
    /// the database calls its own body makes through an ORM: "reads User", "writes database" (filled by analyze)
    pub db: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub name: String,
    /// what it is called on: "self", "Foo", "util", "" for a bare call
    pub recv: String,
    pub at: usize,
    pub end: usize,
    /// where the whole expression starts (`Router()` in `Router().use(a)`)
    pub from: usize,
    pub line: usize,
    /// index of the definition it is written in
    pub def: usize,
    /// a function handed to another call (Thread(target=run)), not called right here
    pub is_ref: bool,
    /// what the call sits under inside its function: "if", "loop", "error", or "" for always
    pub cond: &'static str,
}

/// A web endpoint registered by a call: app.get("/users", listUsers), mux.HandleFunc("/x", h)
#[derive(Clone, Debug)]
pub struct Route {
    pub verb: String,
    pub path: String,
    pub line: usize,
    pub at: usize,
    pub end: usize,
    pub def: usize,
    /// what it is called on: `router` in `router.get("/x", h)`
    pub recv: String,
    /// how the handler is written: `list_items`, `[UserController::class, 'index']` ("" when none is given)
    pub handler: String,
    /// the name views are joined to it by, when it is registered by name (`add_route("users", "/users")`)
    pub name: String,
}

/// (class, function) a handler expression names; the class is "" when only a function is named
pub fn handler_of(expr: &str) -> Option<(String, String)> {
    let mut expr = expr.trim();
    if let Some(c) = HANDLER_WRAP.captures(expr) {
        expr = c.name("inner").map_or("", |m| m.as_str());
    }
    HANDLER_FORMS.iter().find_map(|re| {
        let c = re.captures(expr)?;
        Some((c.name("class").map_or("", |m| m.as_str()).to_string(), c.name("func").map_or("", |m| m.as_str()).to_string()))
    })
}

/// The path a chain of router calls sets for the call made on it: `app.at("/x")` -> "/x", `Router::with_path("x").get(a)` -> "x".
/// Walks back over routes registered on the chain and routers nested in it; any other call ends it (none).
/// Path calls right before the first one found add to it: `app.at("/users").at("/:id")` -> "/users/:id".
pub fn chain_path(lang: &str, recv: &str) -> Option<String> {
    let ch = crate::rules::chain_of(lang)?;
    let mut s = recv.trim_end();
    let mut parts: Vec<String> = vec![];
    // the path calls found so far, innermost last; the walk ends where the next call out is not one
    let joined = |parts: &[String]| {
        let mut rev: Vec<&str> = parts.iter().map(|p| p.as_str()).collect();
        rev.reverse();
        crate::mounts::join_path(&rev)
    };
    loop {
        match last_chain_call(s) {
            Some((name, arg, rest)) if ch.paths.contains(&name) => match one_string(arg) {
                Some(p) => {
                    parts.push(p);
                    s = rest;
                }
                None if parts.is_empty() => return None,
                None => return Some(joined(&parts)),
            },
            _ if !parts.is_empty() => return Some(joined(&parts)),
            Some((name, _, rest)) if ch.verbs.contains(&name) || ch.routes.contains(&name) || ch.nests.contains(&name) => s = rest,
            _ => return None,
        }
    }
}

/// the text of a lone string argument: `"/x"` -> "/x"
fn one_string(arg: &str) -> Option<String> {
    static ONE_STRING: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*"([^"]*)"\s*$"#));
    ONE_STRING.captures(arg).map(|c| c[1].to_string())
}

/// the last call of a chain written `<rest>.name(arg)` or `<rest>::name(arg)`: (name, arg, rest)
fn last_chain_call(s: &str) -> Option<(&str, &str, &str)> {
    let s = s.trim_end();
    let body = s.strip_suffix(')')?;
    // the parenthesis that opens the last call
    let b = body.as_bytes();
    let (mut depth, mut i, mut quoted) = (0i32, b.len(), false);
    let open = loop {
        if i == 0 {
            return None;
        }
        i -= 1;
        match b[i] {
            b'"' => quoted = !quoted,
            b')' if !quoted => depth += 1,
            b'(' if !quoted && depth == 0 => break i,
            b'(' if !quoted => depth -= 1,
            _ => {}
        }
    };
    let head = body[..open].trim_end();
    let rest = head.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    let name = &head[rest.len()..];
    let rest = rest.trim_end();
    let rest = rest.strip_suffix('.').or_else(|| rest.strip_suffix("::"))?;
    Some((name, &body[open + 1..], rest.trim_end()))
}

/// a route's own path under the path of the chain it is registered on
fn under(chain: &str, own: &str) -> String {
    if own.trim_matches('/').is_empty() { crate::mounts::join_path(&[chain]) } else { crate::mounts::join_path(&[chain, own]) }
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub k: &'static str,
    pub n: String,
    pub sig: String,
    pub doc: String,
    pub line: usize,
    pub members: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct PyImport {
    pub level: usize,
    pub module: String,
    pub names: Vec<String>,
    pub is_from: bool,
}

pub struct File {
    pub path: String,
    pub lang: &'static Lang,
    pub text: String,
    pub loc: usize,
    /// the text with comments and docstrings blanked out (byte offsets unchanged)
    pub bare: String,
    /// `bare` with long strings blanked out too
    pub code: String,
    pub defs: Vec<Def>,
    pub calls: Vec<Call>,
    pub routes: Vec<Route>,
    pub symbols: Vec<Symbol>,
    pub doc: String,
    pub entry: bool,
    pub imports: Vec<String>,
    pub py_imports: Vec<PyImport>,
    pub mods: Vec<String>,
    /// names its imports bind: modules, packages and their aliases (`import x as y`, `use a::b`)
    pub aliases: Vec<String>,
    /// JS family: (local name, import path) of default and namespace imports
    pub bindings: Vec<(String, String)>,
    /// local names bound to a whole file of the repo (filled once imports are resolved)
    pub bound: Vec<(String, String)>,
    /// names imported one by one: (local name, module path as written, name in that module)
    pub named: Vec<(String, String, String)>,
    /// `named` once the module is a file of the repo: (local name, name in that file, file)
    pub bound_names: Vec<(String, String, String)>,
    pub pkg: String,
    pub layer: Option<&'static str>,
    pub uses: Vec<String>,
    pub ext: Vec<(String, String)>,
    pub io: Vec<&'static str>,
}

impl File {
    pub fn new(path: String, lang: &'static Lang, text: String, loc: usize) -> File {
        File {
            path, lang, text, loc, bare: String::new(), code: String::new(), defs: vec![], calls: vec![], routes: vec![],
            symbols: vec![], doc: String::new(), entry: false, imports: vec![], py_imports: vec![], mods: vec![],
            aliases: vec![], bindings: vec![], bound: vec![], named: vec![], bound_names: vec![], pkg: String::new(), layer: None, uses: vec![], ext: vec![], io: vec![],
        }
    }

    /// The definition a program starts at when this file is run: its main guard, its main() where the language
    /// starts there, else its top level.
    pub fn entry_def(&self) -> usize {
        if let Some(i) = self.defs.iter().position(|d| d.kind == Kind::Main) {
            return i;
        }
        if self.lang.entry_re.iter().any(|p| p.contains("main") || p.contains("Main")) && self.lang.scope != crate::lang::Scope::Imports
            || self.lang.name == "Rust"
        {
            if let Some(i) = self.defs.iter().position(|d| d.kind == Kind::Func && (d.name == "main" || d.name == "Main")) {
                return i;
            }
        }
        0
    }

    /// A function this file defines at its top level (or a class's method), by name: `module:func` in a command.
    pub fn def_named(&self, name: &str) -> Option<usize> {
        self.defs.iter().position(|d| d.kind == Kind::Func && d.name == name && d.parent == Some(0))
            .or_else(|| self.defs.iter().position(|d| d.kind == Kind::Func && d.name == name))
    }
}

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

/// Replace [a, b) with spaces, keeping newlines and every byte offset.
fn blank(buf: &mut String, a: usize, b: usize) {
    let (mut a, mut b) = (a.min(buf.len()), b.min(buf.len()));
    while a < b && !buf.is_char_boundary(a) {
        a += 1;
    }
    while b > a && !buf.is_char_boundary(b) {
        b -= 1;
    }
    if a >= b {
        return;
    }
    let piece: String = buf[a..b].chars().map(|c| if c == '\n' { "\n".to_string() } else { " ".repeat(c.len_utf8()) }).collect();
    buf.replace_range(a..b, &piece);
}

/// Vue / Svelte: only the <script> blocks are code; the rest becomes spaces so offsets stay right.
fn script_only(text: &str) -> (String, bool) {
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| rx(r"(?is)<script\b([^>]*)>(.*?)</script>"));
    let mut out = text.to_string();
    let mut keep = vec![];
    let mut ts = false;
    for c in BLOCK.captures_iter(text) {
        let body = c.get(2).unwrap();
        keep.push((body.start(), body.end()));
        ts |= c[1].contains("ts");
    }
    let mut at = 0;
    for (a, b) in keep.into_iter().chain([(text.len(), text.len())]) {
        blank(&mut out, at, a);
        at = b;
    }
    (out, ts)
}

/// The "(a, b)" after a definition's name.
fn signature(text: &str, name_end: usize, strip_self: bool) -> String {
    let rest = slice(text, name_end, name_end + 600);
    let Some(open) = rest.find('(').filter(|&i| i < 40 && !rest[..i].contains(['{', '\n', ';'])) else {
        return String::new();
    };
    let mut depth = 0;
    let mut close = None;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else { return String::new() };
    let mut parts: Vec<String> = squash_ws(&rest[open + 1..close]).split(", ").map(String::from).filter(|p| !p.is_empty()).collect();
    if strip_self && parts.first().is_some_and(|p| matches!(p.trim_start_matches(['&', ' ']).trim_start_matches("mut "), "self" | "cls")) {
        parts.remove(0);
    }
    let s = parts.join(", ");
    let s = if s.chars().count() > 70 { format!("{}…", s.chars().take(70).collect::<String>()) } else { s };
    format!("({s})")
}

/// Names after a class's own name: `class A(Base)`, `extends B implements C`, `: Base()`, `< Bar`
fn bases_of(head: &str) -> Vec<String> {
    static IDENT: LazyLock<Regex> = LazyLock::new(|| rx(r"[A-Za-z_][\w.:]*"));
    const SKIP: &[&str] = &["extends", "implements", "public", "private", "protected", "internal", "final", "abstract", "class",
        "struct", "where", "with", "open", "sealed", "data", "object", "metaclass", "virtual", "for", "impl", "new"];
    let mut out: Vec<String> = vec![];
    for m in IDENT.find_iter(head) {
        let last = m.as_str().rsplit(['.', ':']).next().unwrap_or("").to_string();
        if !last.is_empty() && !SKIP.contains(&last.as_str()) && !out.contains(&last) {
            out.push(last);
        }
    }
    out.truncate(6);
    out
}

/// A Python-style docstring: the first statement of a body, when it is a string.
fn docstring(node: Node, src: &str) -> String {
    let body = if node.kind() == "module" { Some(node) } else { node.child_by_field_name("body") };
    let Some(first) = body.and_then(|b| b.named_child(0)) else { return String::new() };
    if first.kind() != "expression_statement" {
        return String::new();
    }
    match first.named_child(0) {
        Some(s) if s.kind() == "string" => {
            let t = &src[s.byte_range()];
            let t = t.trim_start_matches(|c: char| "rRbBuUfF".contains(c));
            first_sentence(t.trim_matches(|c| c == '"' || c == '\''), 150)
        }
        _ => String::new(),
    }
}

/// What a call sits under inside its own function: a branch, a loop or an error handler (nearest wins).
/// Read from the names every grammar gives these nodes; the tests of an `if` / `while` always run, so they do not count.
fn cond_of(node: Node) -> &'static str {
    let mut at = node;
    while let Some(p) = at.parent() {
        let k = p.kind();
        if k.contains("function") || k.contains("lambda") || k.contains("closure") || k.contains("class") || k == "arrow_function"
            || k == "method_definition" || k.ends_with("method_declaration") || k.ends_with("constructor_declaration") || k.ends_with("_item")
        {
            return "";
        }
        let in_test = p.child_by_field_name("condition").is_some_and(|c| c.start_byte() <= node.start_byte() && node.end_byte() <= c.end_byte());
        if !in_test {
            if k.contains("catch") || k.contains("except") || k.contains("rescue") {
                return "error";
            }
            if k.starts_with("for") || k.starts_with("while") || k.contains("loop") || k.starts_with("do_") {
                return "loop";
            }
            if k.starts_with("if") || k.contains("conditional") || k.contains("ternary") || k.contains("match") || k.contains("switch")
                || k.contains("elif") || k.contains("else")
            {
                return "if";
            }
        }
        at = p;
    }
    ""
}

/// Where a definition's decorators / annotations start.
fn head_of(node: Node) -> usize {
    let mut start = node.start_byte();
    if let Some(p) = node.parent().filter(|p| p.kind().contains("decorated")) {
        start = p.start_byte();
    }
    let mut prev = node.prev_named_sibling();
    while let Some(p) = prev {
        if !(p.kind().contains("decorator") || p.kind().contains("annotation") || p.kind().contains("attribute")) {
            break;
        }
        start = start.min(p.start_byte());
        prev = p.prev_named_sibling();
    }
    start
}

fn decorator_route(head: &str) -> Option<(String, String)> {
    static METHODS: LazyLock<Regex> = LazyLock::new(|| rx(r"methods?\s*[=:]\s*[\[(]?([^\])\n]*)"));
    for re in DECORATOR_ROUTES.iter() {
        if let Some(c) = re.captures(head) {
            let mut verb = c[1].to_uppercase();
            if verb == "ROUTE" || verb == "REQUEST" {
                verb = "ANY".into();
                if let Some(m) = METHODS.captures(head) {
                    let vs: Vec<String> = m[1].split(',').map(|v| v.trim().trim_matches(['"', '\'']).to_string()).filter(|v| !v.is_empty()).collect();
                    if !vs.is_empty() {
                        verb = vs.join("|");
                    }
                }
            }
            return Some((verb, c[2].to_string()));
        }
    }
    None
}

/// A verb decorator that names no path, where the language writes one (NestJS `@Post()`): it answers at the class prefix
fn bare_verb_decorator(lang: &str, head: &str) -> Option<(String, String)> {
    if !BARE_VERB_DECORATOR_LANGS.contains(&lang) {
        return None;
    }
    BARE_VERB_DECORATOR.captures(head).map(|c| (c[1].to_uppercase(), String::new()))
}

/// The object a decorator route is registered on: `@router.get("/x")` -> router, `@app.route(..)` -> app
fn decorator_recv(head: &str) -> String {
    static RECV: LazyLock<Regex> = LazyLock::new(|| rx(r"@(?:[\w$]+\.)*([\w$]+)\.(?i:get|post|put|delete|patch|route|head|options|websocket)\("));
    RECV.captures(head).map(|c| c[1].to_string()).unwrap_or_default()
}

/// A view joined to a route by its name: `@view_config(route_name="users", request_method="GET")` -> (users, GET)
fn view_for(head: &str) -> Option<(String, String)> {
    let name = VIEW_ROUTE_NAME.captures(head)?[1].to_string();
    let verb = VIEW_VERB.captures(head).map_or("ANY".to_string(), |c| c[1].to_uppercase());
    Some((name, verb))
}

/// A class-wide route prefix: `@RequestMapping("/api")`, `@Controller('users')`, `@RoutePrefix("api")`
fn class_prefix(head: &str) -> String {
    CLASS_PREFIX.captures(head).map(|c| c[1].to_string()).unwrap_or_default()
}

/// Strip generics and paths from a captured name: `Foo<T>` -> Foo, `Util::go` -> go
fn clean_name(s: &str) -> String {
    let s = s.split('<').next().unwrap_or(s);
    s.rsplit("::").next().unwrap_or(s).trim().to_string()
}

pub fn parse(f: &mut File) {
    let (src, ts_script) = if f.lang.script_blocks { script_only(&f.text) } else { (f.text.clone(), false) };
    let key = lang::grammar_key(f.lang, &f.path, ts_script);
    let base = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
    f.defs.push(Def {
        name: base, kind: Kind::Module, start: 0, end: f.text.len(), line: 1, parent: None, owner: None, bases: vec![],
        sig: String::new(), doc: String::new(), route: None, route_on: String::new(), prefix: String::new(), view_for: None, io: vec![], db: vec![],
    });
    match key.and_then(|k| lang::GRAMMARS.get(k)) {
        Some(g) => parse_tree(f, &src, g),
        None => {
            // a page or style sheet: no syntax tree, only the text
            f.bare = COMMENTS.replace_all(&f.text, " ").into_owned();
            f.code = STRINGS.replace_all(&f.bare, "\"\"").into_owned();
        }
    }
    let lines: Vec<&str> = f.text.lines().collect();
    for d in f.defs.iter_mut().skip(1) {
        if d.doc.is_empty() && f.lang.header_comments {
            d.doc = lead_comment(&lines, line_of(&f.text, d.start).saturating_sub(1));
        }
    }
    if f.doc.is_empty() && f.lang.header_comments {
        f.doc = header_doc(&f.text);
    }
    f.entry = f.text.starts_with("#!")
        || (f.lang.name == "Python" && f.path.ends_with("__main__.py"))
        || (!f.lang.entry_re.is_empty() && f.lang.entry_re.iter().all(|p| Regex::new(p).unwrap().is_match(&f.bare)));
    file_routes(f);
    imports(f);
    symbols(f);
}

/// A file whose place in the tree is its route (rules::FILE_ROUTES): its default export answers every verb, or each
/// function it exports under a verb's name answers that verb.
fn file_routes(f: &mut File) {
    let Some((rule, path)) = crate::rules::file_route(f.lang.name, &f.path) else { return };
    let top = |d: &Def| d.kind == Kind::Func && d.parent == Some(0);
    let route = |verb: &str, at: usize, end: usize, handler: &str| Route {
        verb: verb.to_string(), path: path.clone(), line: line_of(&f.text, at), at, end, def: 0, recv: String::new(),
        handler: handler.to_string(), name: String::new(),
    };
    let mut found = vec![];
    if rule.verb_exports {
        // `export async function GET(req)`, `export const POST = async (req) => ..`
        for d in f.defs.iter().filter(|d| top(d) && crate::rules::ROUTE_EXPORT_VERBS.contains(&d.name.as_str())) {
            let line = f.code[..d.start.min(f.code.len())].rfind('\n').map_or(0, |i| i + 1);
            if f.code[line..d.start.min(f.code.len())].split_whitespace().any(|w| w == "export") {
                found.push(route(&d.name, d.start, d.end, &d.name));
            }
        }
    } else if let Some(c) = crate::rules::DEFAULT_EXPORT.captures(&f.code) {
        let whole = c.get(0).unwrap();
        let name = c.name("func").or(c.name("name")).map_or("", |m| m.as_str());
        match f.defs.iter().find(|d| top(d) && !name.is_empty() && d.name == name) {
            // a function of the file, named: it is the handler
            Some(d) => found.push(route("ANY", d.start, d.end, name)),
            // written in place without a name: the calls inside it
            None if c.name("func").is_some() => {
                let end = body_end(&f.code, whole.end()).unwrap_or(f.code.len());
                found.push(route("ANY", whole.start(), end, ""));
            }
            None => {}
        }
    }
    f.routes.extend(found);
}

/// where the `{..}` body that follows `from` ends
fn body_end(code: &str, from: usize) -> Option<usize> {
    let open = from + code.get(from..)?.find('{')?;
    let mut depth = 0;
    for (i, b) in code.bytes().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_tree(f: &mut File, src: &str, g: &lang::Grammar) {
    static GUARD: LazyLock<Regex> = LazyLock::new(|| {
        rx(r#"^\s*(?:if|unless)\s*\(?\s*(?:__name__\s*==\s*['"]__main__['"]|['"]__main__['"]\s*==\s*__name__|__FILE__\s*==\s*\$(?:0|PROGRAM_NAME)|\$(?:0|PROGRAM_NAME)\s*==\s*__FILE__)"#)
    });
    let mut parser = Parser::new();
    if parser.set_language(&g.language).is_err() {
        return;
    }
    let Some(tree) = parser.parse(src, None) else { return };
    let names = g.query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&g.query, tree.root_node(), src.as_bytes());
    let mut defs: Vec<Def> = vec![];
    let mut seen_defs = HashSet::new();
    let mut raw_calls: Vec<(Node, Node)> = vec![];
    let mut refs: Vec<Node> = vec![];
    let mut comments: Vec<(usize, usize)> = vec![];
    let mut strings: Vec<(usize, usize)> = vec![];
    let docstrings = !f.lang.header_comments;
    if docstrings {
        f.doc = docstring(tree.root_node(), src);
    }
    while let Some(m) = matches.next() {
        let cap = |want: &str| m.captures().iter().find(|c| names[c.index as usize] == want).map(|c| c.node);
        let main = names[m.captures().iter().map(|c| c.index as usize).find(|&i| {
            matches!(names[i], "class" | "type" | "impl" | "func" | "call" | "ref" | "guard" | "comment" | "doc" | "string")
        }).unwrap_or(0)];
        let Some(node) = cap(main) else { continue };
        match main {
            "class" | "type" | "impl" | "func" => {
                let Some(name) = cap("name") else { continue };
                let kind = match main { "class" => Kind::Class, "type" => Kind::Type, "impl" => Kind::Impl, _ => Kind::Func };
                if !seen_defs.insert((node.start_byte(), node.end_byte(), main)) {
                    continue;
                }
                let head = head_of(node);
                let body = node.child_by_field_name("body").map(|b| b.start_byte()).unwrap_or(node.end_byte());
                let route = if kind == Kind::Func {
                    let above = &src[head..name.start_byte()];
                    decorator_route(above).or_else(|| bare_verb_decorator(f.lang.name, above))
                } else {
                    None
                };
                defs.push(Def {
                    name: clean_name(&src[name.byte_range()]),
                    kind,
                    start: node.start_byte(),
                    end: node.end_byte(),
                    line: node.start_position().row + 1,
                    parent: None,
                    owner: cap("owner").map(|o| clean_name(&src[o.byte_range()])),
                    bases: if kind == Kind::Class { bases_of(&src[name.end_byte()..body.max(name.end_byte())]) } else { vec![] },
                    sig: if kind == Kind::Func { signature(src, name.end_byte(), true) } else { String::new() },
                    doc: if docstrings { docstring(node, src) } else { String::new() },
                    route,
                    route_on: if kind == Kind::Func { decorator_recv(&src[head..name.start_byte()]) } else { String::new() },
                    prefix: if kind == Kind::Class { class_prefix(&src[head..name.start_byte()]) } else { String::new() },
                    view_for: if kind == Kind::Func { view_for(&src[head..name.start_byte()]) } else { None },
                    io: vec![],
                    db: vec![],
                });
                // a decorated / annotated definition is described from its first decorator line
                if head < node.start_byte() {
                    defs.last_mut().unwrap().start = head;
                }
            }
            "call" => {
                if let Some(callee) = cap("callee") {
                    raw_calls.push((node, callee));
                }
            }
            "ref" => refs.push(node),
            "guard" => {
                let text = &src[node.byte_range()];
                if GUARD.is_match(text) {
                    let first = text.lines().next().unwrap_or("").trim().trim_end_matches(':').to_string();
                    defs.push(Def {
                        name: first, kind: Kind::Main, start: node.start_byte(), end: node.end_byte(),
                        line: node.start_position().row + 1, parent: None, owner: None, bases: vec![], sig: String::new(),
                        doc: String::new(), route: None, route_on: String::new(), prefix: String::new(), view_for: None, io: vec![], db: vec![],
                    });
                }
            }
            "comment" | "doc" => comments.push((node.start_byte(), node.end_byte())),
            "string" => {
                let t = &src[node.byte_range()];
                // "$1" in a regex replacement is text; in a shell script it is an argument
                let mut w = node.walk();
                let expands = node.named_children(&mut w).any(|k| k.kind().contains("expansion"));
                let placeholder = (t.contains('$') || t.contains('%')) && !expands;
                if node.end_byte() - node.start_byte() >= 6 || t.contains('\n') || placeholder {
                    strings.push((node.start_byte() + 1, node.end_byte().saturating_sub(1)));
                }
            }
            _ => {}
        }
    }

    // nesting: each definition's parent is the innermost one around it (index 0 is the file itself)
    defs.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut stack: Vec<usize> = vec![0];
    for d in defs {
        while stack.len() > 1 && f.defs[*stack.last().unwrap()].end <= d.start {
            stack.pop();
        }
        let i = f.defs.len();
        let parent = *stack.last().unwrap();
        let mut d = d;
        d.parent = Some(parent);
        let p = &f.defs[parent];
        if d.owner.is_none() && d.kind == Kind::Func && matches!(p.kind, Kind::Class | Kind::Type | Kind::Impl) {
            d.owner = Some(p.name.clone());
        }
        f.defs.push(d);
        stack.push(i);
    }

    let mut bare = f.text.clone();
    if f.lang.script_blocks {
        bare = src.to_string();
    }
    for &(a, b) in &comments {
        blank(&mut bare, a, b);
    }
    let mut code = bare.clone();
    for &(a, b) in &strings {
        blank(&mut code, a, b);
    }
    if f.lang.script_blocks {
        // the template is not code, but the paths and names it mentions still link files
        bare = f.text.clone();
        for &(a, b) in &comments {
            blank(&mut bare, a, b);
        }
    }
    f.bare = bare;
    f.code = code;

    let innermost = |at: usize, defs: &[Def]| -> usize {
        let mut best = 0;
        for (i, d) in defs.iter().enumerate().skip(1) {
            if d.start <= at && at < d.end && (d.kind == Kind::Func || d.kind == Kind::Main) {
                best = i;
            }
        }
        best
    };
    let mut seen_calls = HashSet::new();
    for (node, callee) in raw_calls {
        if !seen_calls.insert(callee.start_byte()) {
            continue;
        }
        let name = clean_name(&src[callee.byte_range()]);
        let recv = src[node.start_byte()..callee.start_byte()].trim();
        let recv = recv.trim_end_matches(['.', '?', '!', '&', ':', '-', '>']).trim();
        let recv = recv.trim_start_matches('<'); // <Nav/>
        let recv = if recv == "new" { "" } else { recv };
        let at = callee.start_byte();
        let def = innermost(at, &f.defs);
        let line = callee.start_position().row + 1;
        let in_routes_file = routes_file(&f.path);
        // a whole controller at once: `Route::apiResource('photos', PhotoController::class)`, `router.register(r"users", UserViewSet)`
        if let Some((_, actions)) = RESOURCES.iter().find(|(call, _)| *call == name) {
            static CLASS: LazyLock<Regex> = LazyLock::new(|| rx(r#"(\w+)(?:::class|['"])?\s*$"#));
            let args = crate::mounts::args_of(&src[callee.end_byte()..node.end_byte()]);
            let named = args.first().filter(|a| a.trim_start_matches(|c: char| "rRbBuUfF".contains(c)).starts_with(['"', '\'']));
            if let (Some(path), Some(class)) = (named, args.get(1).and_then(|a| CLASS.captures(a))) {
                let path = path.trim_start_matches(|c: char| "rRbBuUfF".contains(c)).trim_matches(['"', '\'']);
                for (verb, suffix, action) in actions.iter() {
                    f.routes.push(Route {
                        verb: verb.to_string(), path: format!("/{}{suffix}", path.trim_matches('/')), line, at: node.start_byte(), end: node.end_byte(), def,
                        recv: recv.rsplit('.').next().unwrap_or("").to_string(), handler: format!("{}.{action}", &class[1]), name: String::new(),
                    });
                }
            }
        }
        // a router chain: `app.at("/x").get(h)`, `Routes::new().prefix("x").add("/", get(h))` (languages listed in rules::CHAINS)
        let chain = crate::rules::chain_of(f.lang.name);
        let on_chain = chain.and_then(|_| chain_path(f.lang.name, recv));
        let chain_root = || recv.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("").to_string();
        if let (Some(ch), Some(on)) = (chain, &on_chain) {
            let args = crate::mounts::args_of(&src[callee.end_byte()..node.end_byte()]);
            if ch.verbs.contains(&name.as_str()) && args.len() == 1 && !args[0].trim_start().starts_with(['"', '\'']) {
                f.routes.push(Route {
                    verb: name.to_uppercase(), path: under(on, ""), line, at: callee.start_byte(), end: node.end_byte(), def,
                    recv: chain_root(), handler: args[0].clone(), name: String::new(),
                });
            }
        }
        if ROUTE_CALLS.contains(&name.as_str()) && (recv.is_empty() || !ROUTE_BARE.contains(&name.as_str()))
            || recv.is_empty() && ROUTE_BARE.contains(&name.as_str())
            || in_routes_file.is_some_and(|calls| calls.contains(&name.as_str()))
            || on_chain.is_some() && chain.is_some_and(|ch| ch.routes.contains(&name.as_str())) {
            static FIRST: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*\(\s*['"`](?:([A-Z]+)\s+)?(/[^'"`]*)['"`]"#));
            static FIRST_ANY: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*\(\s*r?['"`](?:([A-Z]+)\s+)?([^'"`]*)['"`]"#));
            let re = if in_routes_file.is_some() || on_chain.is_some() { &FIRST_ANY } else { &FIRST };
            if let Some(c) = re.captures(&src[callee.end_byte()..node.end_byte()]) {
                let upper = name.trim_start_matches("add_").to_uppercase();
                let verb = match c.get(1) {
                    Some(v) => v.as_str().to_string(), // "GET /users/{id}": the verb is written in the pattern
                    None if ["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&upper.as_str()) => upper,
                    None => "ANY".to_string(),
                };
                let mut path = c[2].to_string();
                if in_routes_file.is_some() {
                    for (re, to) in PATTERN_PARTS.iter() {
                        path = re.replace_all(&path, *to).into_owned();
                    }
                }
                let path = if path.starts_with('/') { path } else { format!("/{path}") };
                let path = match &on_chain {
                    Some(on) => under(on, &path),
                    None => path,
                };
                let args = crate::mounts::args_of(&src[callee.end_byte()..node.end_byte()]);
                // `app.at("/x").get(h)`, `app.at("/x").at("/y")`: the call only sets the path of what is chained after it
                let sets_path = chain.is_some_and(|ch| {
                    static NEXT: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*\.\s*(\w+)\s*\("));
                    let continues = |n: &str| ch.verbs.contains(&n) || ch.routes.contains(&n) || ch.nests.contains(&n) || ch.paths.contains(&n);
                    ch.paths.contains(&name.as_str()) && args.len() == 1 && NEXT.captures(&src[node.end_byte()..]).is_some_and(|n| continues(&n[1]))
                });
                // the last argument that is not a keyword other than the one that names the handler
                let handler = args.iter().skip(1).rev().find_map(|a| match crate::mounts::kwarg(a) {
                    Some((k, v)) => HANDLER_KWARGS.contains(&k).then(|| v.to_string()),
                    None => Some(a.clone()),
                }).unwrap_or_default();
                let verb = match METHODS_AFTER.captures(&src[node.end_byte()..]) {
                    Some(m) if verb == "ANY" => m[1].to_uppercase(),
                    _ => verb,
                };
                // `.route("/x", get(list))`: the verb is the name of the wrapping call
                let verb = match HANDLER_WRAP.captures(&handler).map(|w| w["wrap"].to_uppercase()) {
                    Some(w) if verb == "ANY" && ["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&w.as_str()) => w,
                    _ => verb,
                };
                // Hono `app.route('/users', users)`: a router handed over under a path is a mount (mounts.rs), not an endpoint
                static ROUTER_NAME: LazyLock<Regex> = LazyLock::new(|| rx(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$"));
                let mounts_router = crate::rules::lang_mount_call(f.lang.name, &name) && args.len() == 2 && ROUTER_NAME.is_match(&args[1]);
                if INCLUDE.is_match(&handler) || sets_path || mounts_router {
                    // `path("api/", include("shop.urls"))` hands over a whole routes file: a mount, not an endpoint
                } else {
                    let recv = if on_chain.is_some() { chain_root() } else { recv.rsplit('.').next().unwrap_or("").to_string() };
                    f.routes.push(Route { verb, path, line, at: callee.start_byte(), end: node.end_byte(), def, recv, handler, name: String::new() });
                }
            }
        }
        // `config.add_route("users", "/users")`: a route by name; the view that names it answers (joined in analyze)
        if NAMED_ROUTE_CALLS.iter().any(|(l, call)| *l == f.lang.name && *call == name) && in_routes_file.is_none() {
            if let Some(c) = NAMED_ROUTE_ARGS.captures(&src[callee.end_byte()..node.end_byte()]) {
                f.routes.push(Route {
                    verb: "ANY".into(), path: c[2].to_string(), line, at: callee.start_byte(), end: node.end_byte(), def,
                    recv: recv.rsplit('.').next().unwrap_or("").to_string(), handler: String::new(), name: c[1].to_string(),
                });
            }
        }
        // `Application([(r"/x", Handler), ..])`: every (path, handler) pair is a route
        if ROUTE_TABLES.iter().any(|(l, call)| *l == f.lang.name && *call == name) {
            let base = callee.end_byte();
            for c in ROUTE_TABLE_ENTRY.captures_iter(&src[base..node.end_byte()]) {
                let mut path = c[1].to_string();
                for (re, to) in PATTERN_PARTS.iter() {
                    path = re.replace_all(&path, *to).into_owned();
                }
                let path = if path.starts_with('/') { path } else { format!("/{path}") };
                let whole = c.get(0).unwrap();
                f.routes.push(Route {
                    verb: "ANY".into(), path, line: line_of(src, base + whole.start()), at: base + whole.start(), end: base + whole.end(), def,
                    recv: String::new(), handler: c[2].to_string(), name: String::new(),
                });
            }
        }
        f.calls.push(Call { name, recv: recv.to_string(), at, end: node.end_byte(), from: node.start_byte(), line, def, is_ref: false, cond: cond_of(node) });
    }
    for node in refs {
        if seen_calls.contains(&node.start_byte()) {
            continue;
        }
        let at = node.start_byte();
        let text = &src[node.byte_range()];
        let cut = [text.rfind("::").map(|i| (i, 2)), text.rfind('.').map(|i| (i, 1))].into_iter().flatten().max();
        let (recv, name) = cut.map(|(i, n)| (&text[..i], &text[i + n..])).unwrap_or(("", text)); // items.router, parse::parse
        f.calls.push(Call {
            name: name.to_string(), recv: recv.to_string(), at, end: node.end_byte(), from: node.start_byte(),
            line: node.start_position().row + 1, def: innermost(at, &f.defs), is_ref: true, cond: cond_of(node),
        });
    }
    f.calls.sort_by_key(|c| c.at);
}

/// Import specs, read from the comment-free text with each language's import syntax.
fn imports(f: &mut File) {
    static PY_FROM: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^[ \t]*from[ \t]+(\.*)([\w.]*)[ \t]+import[ \t]+(\([^)]*\)|[^\n]+)"));
    static PY_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
        rx(r"(?m)^[ \t]*import[ \t]+([\w.]+(?:[ \t]+as[ \t]+\w+)?(?:[ \t]*,[ \t]*[\w.]+(?:[ \t]+as[ \t]+\w+)?)*)")
    });
    static HASH_COMMENT: LazyLock<Regex> = LazyLock::new(|| rx(r"#[^\n]*"));
    static JS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        vec![
            rx(r#"\bimport\s+(?:[^'"();]*?\sfrom\s+)?['"]([^'"\n]+)['"]"#),
            rx(r#"\bexport\s+(?:type\s+)?(?:\*(?:\s+as\s+\w+)?|\{[^}]*\})\s*from\s*['"]([^'"\n]+)['"]"#),
            rx(r#"\brequire\(\s*['"]([^'"\n]+)['"]\s*\)"#),
            rx(r#"\bimport\(\s*['"]([^'"\n]+)['"]\s*\)"#),
        ]
    });
    static GO_BLOCK: LazyLock<Regex> = LazyLock::new(|| rx(r"(?ms)^import\s*\((.*?)^\)"));
    static GO_STR: LazyLock<Regex> = LazyLock::new(|| rx(r#""([^"]+)""#));
    static GO_ONE: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?m)^import\s+(?:\w+\s+)?"([^"]+)""#));
    static RS_MOD: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;"));
    static RS_USE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([\w:]+)"));
    static JVM_PKG: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*package\s+([\w.]+)"));
    static JVM_IMPORT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*import\s+(?:static\s+)?([\w.]+)"));
    static C_INCLUDE: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?m)^\s*#\s*include\s+"([^"]+)""#));
    let t = &f.bare;
    match f.lang.resolver {
        Resolver::Python => {
            for c in PY_FROM.captures_iter(t) {
                let body = HASH_COMMENT.replace_all(&c[3], "");
                let names: Vec<String> = body
                    .trim_matches(|ch: char| "() \t\n".contains(ch))
                    .split(',')
                    .filter_map(|n| n.split_whitespace().next().map(String::from))
                    .collect();
                f.py_imports.push(PyImport { level: c[1].len(), module: c[2].to_string(), names, is_from: true });
            }
            for c in PY_IMPORT.captures_iter(t) {
                for part in c[1].split(',') {
                    let ws: Vec<&str> = part.split_whitespace().collect();
                    if let Some(m) = ws.first() {
                        f.py_imports.push(PyImport { level: 0, module: m.to_string(), names: vec![], is_from: false });
                        f.aliases.push(if ws.len() == 3 { ws[2].to_string() } else { m.split('.').next().unwrap_or(m).to_string() });
                    }
                }
            }
            for c in PY_FROM.captures_iter(t) {
                let body = HASH_COMMENT.replace_all(&c[3], "");
                for n in body.trim_matches(|ch: char| "() \t\n".contains(ch)).split(',') {
                    let ws: Vec<&str> = n.split_whitespace().collect();
                    if let Some(last) = ws.last() {
                        f.aliases.push(last.to_string());
                        if ws[0] != "*" {
                            f.named.push((last.to_string(), format!("{}{}", ".".repeat(c[1].len()), &c[2]), ws[0].to_string()));
                        }
                    }
                }
            }
        }
        Resolver::Js => {
            static BIND: LazyLock<Regex> = LazyLock::new(|| {
                rx(r#"\bimport\s+(?:\*\s+as\s+)?([A-Za-z_$][\w$]*)\s*(?:,|\s+from\b)|\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*require\("#)
            });
            f.imports = JS.iter().flat_map(|re| re.captures_iter(t).map(|c| c[1].to_string())).collect();
            f.aliases = BIND.captures_iter(t).filter_map(|c| c.get(1).or(c.get(2)).map(|m| m.as_str().to_string())).collect();
            static SPEC: LazyLock<Regex> = LazyLock::new(|| {
                rx(r#"\bimport\s+(?:\*\s+as\s+)?([A-Za-z_$][\w$]*)\s*(?:,\s*\{[^}]*\}\s*)?from\s*['"]([^'"\n]+)['"]|\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*require\(\s*['"]([^'"\n]+)['"]\s*\)"#)
            });
            static NAMED: LazyLock<Regex> = LazyLock::new(|| {
                rx(r#"\b(?:import|export)\s+(?:type\s+)?(?:[A-Za-z_$][\w$]*\s*,\s*)?\{([^}]*)\}\s*from\s*['"]([^'"\n]+)['"]"#)
            });
            for c in NAMED.captures_iter(t) {
                for item in c[1].split(',') {
                    let ws: Vec<&str> = item.split_whitespace().filter(|w| *w != "type").collect();
                    match ws.as_slice() {
                        [a] => f.named.push((a.to_string(), c[2].to_string(), a.to_string())),
                        [a, "as", b] => f.named.push((b.to_string(), c[2].to_string(), a.to_string())),
                        _ => {}
                    }
                }
            }
            f.bindings = SPEC.captures_iter(t)
                .filter_map(|c| match (c.get(1).or(c.get(3)), c.get(2).or(c.get(4))) {
                    (Some(a), Some(s)) => Some((a.as_str().to_string(), s.as_str().to_string())),
                    _ => None,
                })
                .collect();
        }
        Resolver::Go => {
            static GO_SPEC: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?m)^\s*(?:import\s+)?(\w+\s+)?"([^"]+)""#));
            for b in GO_BLOCK.captures_iter(t) {
                f.imports.extend(GO_STR.captures_iter(&b[1]).map(|c| c[1].to_string()));
                for c in GO_SPEC.captures_iter(&b[1]) {
                    f.aliases.push(c.get(1).map(|a| a.as_str().trim().to_string()).unwrap_or_else(|| c[2].rsplit('/').next().unwrap_or("").to_string()));
                }
            }
            for c in GO_ONE.captures_iter(t) {
                f.imports.push(c[1].to_string());
                f.aliases.push(c[1].rsplit('/').next().unwrap_or("").to_string());
            }
        }
        Resolver::Rust => {
            static RS_USE_FULL: LazyLock<Regex> = LazyLock::new(|| rx(r"(?ms)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);"));
            static RS_BIND: LazyLock<Regex> = LazyLock::new(|| rx(r"(\w+)(?:\s*::\s*\{([^{}]*)\})?(?:\s+as\s+(\w+))?\s*(?:,|\}|$)"));
            f.mods = RS_MOD.captures_iter(t).map(|c| c[1].to_string()).collect();
            f.imports = RS_USE.captures_iter(t).map(|c| c[1].trim_matches(':').to_string()).collect();
            f.aliases = f.mods.clone();
            for u in RS_USE_FULL.captures_iter(t) {
                rust_named(&u[1], &mut f.named);
                // use crate::graph::{self, Gid} binds graph; use a::b as c binds c
                for c in RS_BIND.captures_iter(&u[1]) {
                    let name = c.get(3).map(|m| m.as_str()).unwrap_or(&c[1]);
                    if c.get(2).is_some_and(|inner| inner.as_str().split(',').any(|x| x.trim() == "self")) {
                        f.aliases.push(c[1].to_string());
                    }
                    if name != "self" && name != "crate" && name != "super" {
                        f.aliases.push(name.to_string());
                    }
                }
            }
        }
        Resolver::Jvm => {
            f.pkg = JVM_PKG.captures(t).map(|c| c[1].to_string()).unwrap_or_default();
            f.imports = JVM_IMPORT.captures_iter(t).map(|c| c[1].trim_end_matches('.').to_string()).collect();
            f.aliases = f.imports.iter().map(|i| i.rsplit('.').next().unwrap_or("").to_string()).collect();
        }
        Resolver::Php => {
            static PHP_USE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\s*use\s+(?:function\s+|const\s+)?\\?([\w\\]+)(?:\s+as\s+\w+)?\s*;"));
            f.imports = PHP_USE.captures_iter(t).map(|c| c[1].to_string()).collect();
            f.aliases = f.imports.iter().map(|i| i.rsplit('\\').next().unwrap_or("").to_string()).collect();
        }
        Resolver::C => f.imports = C_INCLUDE.captures_iter(t).map(|c| c[1].to_string()).collect(),
        Resolver::None => {}
    }
}

/// `use a::b::{c, d as e}` / `use a::b::c as d` -> (local, "a::b", remote) for each name
fn rust_named(spec: &str, out: &mut Vec<(String, String, String)>) {
    let spec: String = spec.split_whitespace().collect::<Vec<_>>().join(" ");
    let (prefix, items): (String, Vec<String>) = match spec.split_once('{') {
        Some((p, rest)) if !rest.contains('{') => (p.trim().trim_end_matches("::").to_string(), rest.trim_end_matches('}').split(',').map(|x| x.trim().to_string()).collect()),
        Some(_) => return,
        None => match spec.rsplit_once("::") {
            Some((p, last)) => (p.to_string(), vec![last.to_string()]),
            None => return,
        },
    };
    if prefix.is_empty() || ["std", "core", "alloc"].contains(&prefix.split("::").next().unwrap_or("")) {
        return;
    }
    for it in items {
        let ws: Vec<&str> = it.split_whitespace().collect();
        let (remote, local) = match ws.as_slice() {
            [a] => (*a, *a),
            [a, "as", b] => (*a, *b),
            _ => continue,
        };
        if !["self", "*", "crate", "super"].contains(&remote) && !remote.is_empty() {
            out.push((local.to_string(), prefix.clone(), remote.to_string()));
        }
    }
}

/// What the file page lists: its classes (with their methods), top-level functions and web endpoints.
fn symbols(f: &mut File) {
    let jsx = f.path.ends_with(".jsx") || f.path.ends_with(".tsx");
    let in_func = |i: usize| {
        let mut p = f.defs[i].parent;
        while let Some(j) = p {
            if f.defs[j].kind == Kind::Func {
                return true;
            }
            p = f.defs[j].parent;
        }
        false
    };
    let mut out = vec![];
    for (i, d) in f.defs.iter().enumerate().skip(1) {
        let top_func = d.kind == Kind::Func && d.parent == Some(0);
        let container = matches!(d.kind, Kind::Class | Kind::Type) && !in_func(i);
        if !(top_func || container) {
            continue;
        }
        let (k, n, sig) = match (d.kind, &d.route) {
            (Kind::Func, Some((verb, path))) if !f.lang.regex_routes => ("route", path.clone(), verb.clone()),
            (Kind::Func, _) if jsx && d.name.starts_with(|c: char| c.is_ascii_uppercase()) => ("component", d.name.clone(), d.sig.clone()),
            (Kind::Func, _) => ("function", d.name.clone(), d.owner.as_ref().map(|o| format!("on {o}")).unwrap_or_else(|| d.sig.clone())),
            (Kind::Type, _) => ("type", d.name.clone(), String::new()),
            _ => ("class", d.name.clone(), if d.bases.is_empty() { String::new() } else { format!("({})", d.bases.join(", ")) }),
        };
        let members = (d.kind == Kind::Class).then(|| {
            let mut ms: Vec<String> = f.defs.iter()
                .filter(|m| m.kind == Kind::Func && m.owner.as_deref() == Some(d.name.as_str()) && !m.name.starts_with('_'))
                .map(|m| m.name.clone())
                .collect();
            ms.dedup();
            ms.truncate(12);
            ms
        });
        out.push(Symbol { k, n, sig, doc: d.doc.clone(), line: d.line, members });
    }
    if f.lang.regex_routes {
        for re in ROUTE_RES.iter() {
            for c in re.captures_iter(&f.bare) {
                let (verb, path) = if c.len() == 3 { (c[1].to_uppercase(), c[2].to_string()) } else { ("ANY".into(), c[1].to_string()) };
                let whole = c.get(0).unwrap();
                if path.starts_with('/') || whole.as_str().contains("->") || whole.as_str().contains("::") {
                    out.push(Symbol { k: "route", n: path, sig: verb, doc: String::new(), line: line_of(&f.text, whole.start()), members: None });
                }
            }
        }
    }
    out.sort_by_key(|s| s.line);
    let go = f.lang.name == "Go";
    out.sort_by_key(|s| (s.k != "route" && s.n.starts_with('_')) || (go && s.k != "route" && !s.n.starts_with(|c: char| c.is_uppercase())));
    out.truncate(60);
    f.symbols = out;
}
