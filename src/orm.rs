//! Database access through an ORM: which classes are models, which variables hold a database session, and whether
//! a call reads or writes. What each ORM looks like is data (rules.rs); this reads it.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::parse::{Def, File, Kind};
use crate::rules::{orm_form, orm_speaks, Orm, ORMS, ORM_IMPORTED_HANDLE_LANGS, ORM_JS_SYNTAX};

/// the ORMs a file is written for: its language, and it mentions one of their libraries
fn mine(f: &File) -> Vec<(usize, &'static Orm)> {
    ORMS.iter().enumerate().filter(|(_, o)| orm_speaks(o.lang, f.lang.name) && o.libs.iter().any(|l| f.bare.contains(l))).collect()
}

/// models made by a call: `const Post = sequelize.define('Post', ..)`, `module.exports = mongoose.model('User', schema)`,
/// `model<IUser>('User', schema)`:
/// the variable it is put in, else the name it is given
fn called_models(f: &File, o: &Orm) -> Vec<String> {
    let Some(calls) = orm_form(o.name).map(|x| x.model_calls).filter(|c| !c.is_empty()) else { return vec![] };
    let names = calls.iter().map(|c| regex::escape(c)).collect::<Vec<_>>().join("|");
    let re = Regex::new(&format!(r#"(?:(?:^|[^.\w$])([A-Za-z_$][\w$]*)\s*=\s*)?(?:[\w$]+\.)?\b(?:{names})\s*(?:<[^<>()]*>)?\s*\(\s*['"]([A-Za-z_$][\w$]*)['"]"#)).unwrap();
    // a model is a class-like name: the variable when it is spelt so, else the name given (`const user = model('User')` is User)
    let class_like = |m: &regex::Match| m.as_str().starts_with(|c: char| c.is_ascii_uppercase());
    re.captures_iter(&f.bare).filter_map(|c| c.get(1).filter(class_like).or_else(|| c.get(2).filter(class_like)).map(|m| m.as_str().to_string())).collect()
}

/// the attribute lines right above a definition and the start of its body: where `#[derive(..)]`, `@Entity`, `gorm.Model` sit
fn head_text(f: &File, d: &Def) -> String {
    let at = d.start.min(f.bare.len());
    let mut lines: Vec<&str> = vec![];
    for l in f.bare[..at].rsplit('\n').skip(1) {
        let t = l.trim();
        if t.starts_with("#[") || t.starts_with('@') || t.starts_with("//") || t.is_empty() || t.ends_with(")]") {
            lines.push(l);
        } else {
            break;
        }
    }
    lines.reverse();
    format!("{}\n{}", lines.join("\n"), crate::text::slice(&f.bare, at, (at + 400).min(f.bare.len())))
}

fn last_segment(s: &str) -> &str {
    s.rsplit(['.', '\\', ':']).next().unwrap_or(s)
}

/// model class name -> the ORM (index into ORMS) it belongs to. A class is a model when it extends a model base or
/// carries a model marker in a file written for that ORM, or extends another model (`class User(Base)`).
pub fn models(files: &[File]) -> HashMap<String, usize> {
    let mut out: HashMap<String, usize> = HashMap::new();
    let gated: Vec<Vec<(usize, &Orm)>> = files.iter().map(mine).collect();
    for _ in 0..4 {
        let before = out.len();
        for (f, orms) in files.iter().zip(&gated) {
            let mut found: Vec<(String, usize)> = vec![];
            for (oi, o) in orms {
                found.extend(called_models(f, o).into_iter().filter(|n| !out.contains_key(n)).map(|n| (n, *oi)));
            }
            for d in f.defs.iter().filter(|d| matches!(d.kind, Kind::Class | Kind::Type) && !out.contains_key(&d.name)) {
                let head = if orms.is_empty() { String::new() } else { head_text(f, d) };
                let from_root = orms.iter().find(|(_, o)| {
                    d.bases.iter().any(|b| o.model_bases.contains(&last_segment(b))) || o.model_attrs.iter().any(|a| head.contains(a))
                });
                let from_model = d.bases.iter().find_map(|b| out.get(last_segment(b)).copied());
                if let Some(oi) = from_root.map(|(oi, _)| *oi).or(from_model) {
                    found.push((d.name.clone(), oi));
                }
            }
            out.extend(found);
        }
        if out.len() == before {
            break;
        }
    }
    out
}

/// a function that hands out a database session, and where it is declared
pub struct Getter {
    pub orm: usize,
    /// the file it is declared in
    pub path: String,
    /// the names a call qualifies it with: its folder (a Go package) and its file stem (a module): `common.GetDB()`, `db::pool()`
    pub places: Vec<String>,
    /// a method of a class: called on an object (`self.db()`, `$this->em()`), any qualifier is that object
    pub method: bool,
}

impl Getter {
    /// `x := q.Name(..)`, written in `path`: whether it calls this function and not another of the same name
    fn called(&self, qualifier: Option<&str>, path: &str, own_def: bool) -> bool {
        match qualifier {
            _ if self.method => true,
            Some(q) => self.places.iter().any(|p| p == q.trim_start_matches('$')),
            // unqualified: this one, unless the calling file declares its own function of that name
            None => path == self.path || !own_def,
        }
    }
}

/// what the repo's ORM-related names are: model classes, and functions that hand out a database session
pub struct Index {
    pub models: HashMap<String, usize>,
    /// `func GetDB() *gorm.DB`, `def get_db() -> Session`: name -> every function of that name that hands one out
    pub getters: HashMap<String, Vec<Getter>>,
    /// names declared as a session anywhere (a struct field `db: PgPool`): they hold one in the other files too
    pub names: Vec<(String, usize)>,
    /// where handles travel by import (rules::ORM_IMPORTED_HANDLE_LANGS): (file, handle name it declares, ORM)
    pub exported: Vec<(String, String, usize)>,
    /// ... and (file, class it declares that extends a handle type, ORM)
    pub derived: Vec<(String, String, usize)>,
}

pub fn index(files: &[File]) -> Index {
    let mut getters: HashMap<String, Vec<Getter>> = HashMap::new();
    for f in files {
        for (oi, o) in mine(f) {
            for d in f.defs.iter().filter(|d| d.kind == Kind::Func) {
                let line = f.bare[d.start.min(f.bare.len())..].lines().next().unwrap_or("");
                // the type after the parameter list is the one handed back
                let Some(open) = line.find('(') else { continue };
                let (mut depth, mut close) = (0, None);
                for (i, c) in line[open..].char_indices() {
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
                if close.is_some_and(|c| o.handles.iter().any(|h| line[c..].contains(h))) {
                    let dir = f.path.rsplit_once('/').map_or("", |(d, _)| d);
                    let stem = crate::resolve::basename(&f.path).split('.').next().unwrap_or("");
                    let places = [crate::resolve::basename(dir), stem].into_iter().filter(|p| !p.is_empty()).map(String::from).collect();
                    getters.entry(d.name.clone()).or_default().push(Getter { orm: oi, path: f.path.clone(), places, method: d.owner.is_some() });
                }
            }
        }
    }
    let mut names: Vec<(String, usize)> = files.iter().flat_map(|f| handles(f, &getters)).collect();
    names.sort();
    names.dedup();
    let (mut exported, mut derived) = (vec![], vec![]);
    for f in files.iter().filter(|f| ORM_IMPORTED_HANDLE_LANGS.contains(&f.lang.name)) {
        exported.extend(handles(f, &getters).into_iter().map(|(n, oi)| (f.path.clone(), n, oi)));
        for (oi, o) in mine(f) {
            for d in f.defs.iter().filter(|d| d.kind == Kind::Class && d.bases.iter().any(|b| o.handles.contains(&last_segment(b)))) {
                derived.push((f.path.clone(), d.name.clone(), oi));
            }
        }
    }
    Index { models: models(files), getters, names, exported, derived }
}

/// the names that hold a session in this file: its own, and those declared elsewhere for an ORM it is written for
pub fn sessions(f: &File, ix: &Index) -> Vec<(String, usize)> {
    let mut out = handles(f, &ix.getters);
    let mine: Vec<usize> = mine(f).into_iter().map(|(oi, _)| oi).collect();
    out.extend(ix.names.iter().filter(|(_, oi)| mine.contains(oi)).cloned());
    // `import { prisma } from './lib/prisma'`, `constructor(private prisma: PrismaService)` with PrismaService imported:
    // the name, imported from the file that declares it, is the handle declared there
    if ORM_IMPORTED_HANDLE_LANGS.contains(&f.lang.name) {
        for (local, remote, path) in &f.bound_names {
            out.extend(ix.exported.iter().filter(|(p, n, _)| p == path && n == remote).map(|(_, _, oi)| (local.clone(), *oi)));
            for (_, _, oi) in ix.derived.iter().filter(|(p, n, _)| p == path && n == remote) {
                out.extend(typed(f, &ORMS[*oi], local).into_iter().map(|n| (n, *oi)));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// names a file holds a value of type `ty` in, a handle of ORM `o`: `db: Session`, `q *db.Queries`, `Connection $pdo`, `s = Session(..)`
fn typed(f: &File, o: &Orm, ty: &str) -> Vec<String> {
    const NOT_NAMES: &[&str] = &["return", "var", "func", "type", "struct", "chan", "go", "case", "range", "new", "mut", "let", "const", "use",
        "await", "extends", "implements", "typeof", "import", "export", "from", "default"];
    // `const prisma = new PrismaClient()`
    let new_kw = if ORM_JS_SYNTAX.contains(&f.lang.name) { r"(?:new\s+)?" } else { "" };
    let mut out = vec![];
    let ty = regex::escape(ty);
    let forms = [
        format!(r"(\$?\w+)\s*:\s*(?:&\s*(?:mut\s+)?|\*)?(?P<q>(?:\w+(?:\.|::))*){ty}\b"),
        // `q *db.Queries`: the type may be named through its package
        format!(r"(\$?\w+)\s+\*?(?:\w+\.)*{ty}\b"),
        format!(r"\b{ty}\s+(\$\w+)"),
        format!(r"(\$?\w+)\s*:?=\s*{new_kw}(?P<q>(?:\w+(?:\.|::))*){ty}\s*[(\[{{]"),
    ];
    // `http: reqwest::Client`: a type named through a crate path is this ORM's only when the crate is
    // or wraps its handle (`r2d2::Pool`); a path inside the project, `crate::db::Pool`, says nothing either way
    let own_crate = |q: &str| {
        let Some((first, _)) = q.split_once("::") else { return true };
        let first = first.replace('-', "_");
        ["crate", "self", "super"].contains(&first.as_str()) || o.libs.iter().chain(o.pools).any(|l| l.replace('-', "_") == first)
    };
    for re in forms {
        for c in Regex::new(&re).unwrap().captures_iter(&f.code) {
            if !NOT_NAMES.contains(&&c[1]) && c.name("q").is_none_or(|q| own_crate(q.as_str())) {
                out.push(c[1].trim_start_matches('$').to_string());
            }
        }
    }
    out
}

/// names a file holds a database session in: `db: Session`, `pool: &PgPool`, `db *gorm.DB`, `Connection $pdo`, `s = Session(..)`
pub fn handles(f: &File, getters: &HashMap<String, Vec<Getter>>) -> Vec<(String, usize)> {
    let js = ORM_JS_SYNTAX.contains(&f.lang.name);
    let mut out = vec![];
    for (oi, o) in mine(f) {
        if js && orm_form(o.name).is_some_and(|x| x.called_handle) {
            // `const knex = require('knex')({ client: 'pg' })`: the library, called, is the handle
            for l in o.libs {
                let re = Regex::new(&format!(r#"([A-Za-z_$][\w$]*)\s*=\s*require\(\s*['"]{}['"]\s*\)\s*\("#, regex::escape(l))).unwrap();
                out.extend(re.captures_iter(&f.bare).map(|c| (c[1].to_string(), oi)));
            }
        }
        for ty in o.handles {
            out.extend(typed(f, o, ty).into_iter().map(|n| (n, oi)));
        }
    }
    // `db := common.GetDB()`: the function says what it hands back, when the call names that function
    // (`errors.New()` is not the `New` of another package)
    if !getters.is_empty() {
        let names = getters.keys().map(|n| regex::escape(n)).collect::<Vec<_>>().join("|");
        let re = Regex::new(&format!(r"(\$?\w+)\s*(?:,\s*\w+\s*)?:?=\s*((?:\$?\w+(?:\.|::|->))*)({names})\s*\(")).unwrap();
        for c in re.captures_iter(&f.code) {
            let qualifier = c[2].trim_end_matches(['.', ':', '-', '>']).rsplit(['.', ':', '>']).next().filter(|q| !q.is_empty());
            let own_def = f.defs.iter().any(|d| d.kind == Kind::Func && d.name == c[3]);
            if let Some(g) = getters[&c[3]].iter().find(|g| g.called(qualifier, &f.path, own_def)) {
                out.push((c[1].trim_start_matches('$').to_string(), g.orm));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `.objects.filter(a=1).first()` after a receiver: the names called, and the text inside every pair of parentheses
/// Names may hold `$` where `dollar` (JavaScript: `prisma.$queryRaw`).
fn chain(code: &str, mut i: usize, dollar: bool) -> (Vec<&str>, String) {
    let b = code.as_bytes();
    let (mut methods, mut args) = (vec![], String::new());
    let skip = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        j
    };
    loop {
        let mut j = skip(i);
        j += if code[j..].starts_with("->") || code[j..].starts_with("::") {
            2
        } else if code[j..].starts_with('.') {
            1
        } else {
            break;
        };
        j = skip(j);
        let s = j;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || dollar && b[j] == b'$') {
            j += 1;
        }
        if j == s {
            break;
        }
        methods.push(&code[s..j]);
        if b.get(j) == Some(&b'!') && b.get(j + 1) == Some(&b'(') {
            j += 1; // a macro: sqlx::query!(..)
        }
        if code[j..].starts_with("::<") {
            // a turbofish: load::<User>(conn)
            let mut depth = 0;
            while j < b.len() {
                match b[j] {
                    b'<' => depth += 1,
                    b'>' => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        if b.get(j) == Some(&b'(') {
            let (mut depth, mut k) = (0i32, j);
            while k < b.len() {
                match b[k] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            args.push_str(&code[j + 1..k.min(code.len())]);
            args.push(' ');
            j = k + 1;
        }
        i = j;
    }
    (methods, args)
}

/// `reads User`, `writes database`: every database call in `code`, in the order the code makes them
pub fn accesses(code: &str, lang: &str, handles: &[(String, usize)], models: &HashMap<String, usize>) -> Vec<String> {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$?[A-Za-z_]\w*").unwrap());
    static ASSIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\$?[A-Za-z_]\w*)\s*(?::=|=)\s*(?:new\s+)?(\$?[A-Za-z_]\w*)").unwrap());
    // `const user = await User.findByPk(1)`
    static ASSIGN_AWAIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\$?[A-Za-z_]\w*)\s*=\s*(?:await\s+)?(?:new\s+)?(\$?[A-Za-z_]\w*)").unwrap());
    if models.is_empty() && handles.is_empty() {
        return vec![];
    }
    let js = ORM_JS_SYNTAX.contains(&lang);
    let assign = if js { &ASSIGN_AWAIT } else { &ASSIGN };
    // variables holding one instance of a model: `user = User.objects.get(..)`, `$u = new User`
    let instances: HashMap<&str, &str> = assign.captures_iter(code).filter(|c| models.contains_key(&c[2])).map(|c| (c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str())).collect();
    let mut out: Vec<String> = vec![];
    for m in WORD.find_iter(code) {
        let w = m.as_str();
        let prev = code[..m.start()].chars().next_back();
        let member = matches!(prev, Some('.')) || code[..m.start()].ends_with("->") || code[..m.start()].ends_with("::");
        let handle = handles.iter().find(|(n, _)| n == w.trim_start_matches('$'));
        let form = handle.and_then(|(_, oi)| orm_form(ORMS[*oi].name));
        // `db('users').where(..)`: the handle is called first, then the chain
        let after = match (form.is_some_and(|x| x.called_handle) && !member, code[m.end()..].starts_with('(')) {
            (true, true) => close_paren(code, m.end()).unwrap_or(m.end()),
            _ => m.end(),
        };
        let (methods, args) = chain(code, after, js);
        if methods.is_empty() {
            continue;
        }
        let verbs = |oi: usize| {
            let o = &ORMS[oi];
            // a generated name: the prefix, then the next word (`GetUser`, not `Get` or `Getaway`)
            let starts = |x: &str, prefixes: &[&str]| {
                prefixes.iter().any(|p| x.strip_prefix(p).is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase())))
            };
            let write = methods.iter().any(|x| o.writes.contains(x) || starts(x, o.write_prefixes));
            let read = methods.iter().any(|x| o.reads.contains(x) || starts(x, o.read_prefixes));
            (read, write)
        };
        let label = if let (Some(&oi), false) = (models.get(w), member) {
            let (read, write) = verbs(oi);
            (read || write).then(|| (write, w.to_string()))
        } else if let Some((_, oi)) = handle {
            let (read, write) = verbs(*oi);
            let o = &ORMS[*oi];
            // `prisma.orderItem.findMany()`: the member called through is the model, spelt as its class is
            let member_model = methods.first().filter(|x| {
                form.is_some_and(|f| f.member_models) && methods.len() > 1 && !o.reads.contains(x) && !o.writes.contains(x)
                    && x.starts_with(|c: char| c.is_ascii_lowercase())
            }).map(|x| format!("{}{}", x[..1].to_ascii_uppercase(), &x[1..]));
            // where the model is the member, a query is `prisma.<model>.<verb>()` or a `$` call on the client itself (`prisma.$queryRaw`)
            let unshaped = form.is_some_and(|f| f.member_models) && member_model.is_none() && !methods[0].starts_with('$');
            let model = member_model.unwrap_or_else(|| WORD.find_iter(&args).map(|x| x.as_str()).find(|x| models.contains_key(*x)).unwrap_or("database").to_string());
            ((read || write) && !unshaped).then_some((write, model))
        } else if let (Some(model), false) = (instances.get(w), member) {
            let (_, write) = verbs(models[*model]);
            write.then(|| (true, model.to_string()))
        } else if member {
            None
        } else {
            // the session is handed to the call that runs the query: `sqlx::query(..).fetch_all(pool)`, `users.load(conn)`
            let words: Vec<&str> = WORD.find_iter(&args).map(|x| x.as_str()).collect();
            handles.iter().find(|(n, _)| words.iter().any(|x| x.trim_start_matches('$') == n)).and_then(|(_, oi)| {
                let (read, write) = verbs(*oi);
                let model = words.iter().copied().find(|x| models.contains_key(*x)).unwrap_or("database");
                (read || write).then(|| (write, model.to_string()))
            })
        };
        if let Some((write, model)) = label {
            let l = format!("{} {model}", if write { "writes" } else { "reads" });
            if !out.contains(&l) {
                out.push(l);
            }
        }
    }
    out
}

/// just past the parenthesis that closes the one opening at `open`
fn close_paren(code: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, b) in code.bytes().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
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
