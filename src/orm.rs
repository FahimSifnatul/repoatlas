//! Database access through an ORM: which classes are models, which variables hold a database session, and whether
//! a call reads or writes. What each ORM looks like is data (rules.rs); this reads it.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::parse::{Def, File, Kind};
use crate::rules::{Orm, ORMS};

/// the ORMs a file is written for: its language, and it mentions one of their libraries
fn mine(f: &File) -> Vec<(usize, &'static Orm)> {
    ORMS.iter().enumerate().filter(|(_, o)| o.lang == f.lang.name && o.libs.iter().any(|l| f.bare.contains(l))).collect()
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

/// names a file holds a database session in: `db: Session`, `pool: &PgPool`, `db *gorm.DB`, `Connection $pdo`, `s = Session(..)`
pub fn handles(f: &File) -> Vec<(String, usize)> {
    const NOT_NAMES: &[&str] = &["return", "var", "func", "type", "struct", "chan", "go", "case", "range", "new", "mut", "let", "const", "use"];
    let mut out = vec![];
    for (oi, o) in mine(f) {
        for ty in o.handles {
            let ty = regex::escape(ty);
            let forms = [
                format!(r"(\$?\w+)\s*:\s*(?:&\s*(?:mut\s+)?|\*)?(?:\w+(?:\.|::))*{ty}\b"),
                format!(r"(\$?\w+)\s+\*?{ty}\b"),
                format!(r"\b{ty}\s+(\$\w+)"),
                format!(r"(\$?\w+)\s*:?=\s*(?:\w+(?:\.|::))*{ty}\s*[(\[{{]"),
            ];
            for re in forms {
                for c in Regex::new(&re).unwrap().captures_iter(&f.code) {
                    if !NOT_NAMES.contains(&&c[1]) {
                        out.push((c[1].to_string(), oi));
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `.objects.filter(a=1).first()` after a receiver: the names called, and the text inside every pair of parentheses
fn chain(code: &str, mut i: usize) -> (Vec<&str>, String) {
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
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
            j += 1;
        }
        if j == s {
            break;
        }
        methods.push(&code[s..j]);
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
pub fn accesses(code: &str, handles: &[(String, usize)], models: &HashMap<String, usize>) -> Vec<String> {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$?[A-Za-z_]\w*").unwrap());
    static ASSIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\$?[A-Za-z_]\w*)\s*(?::=|=)\s*(?:new\s+)?(\$?[A-Za-z_]\w*)").unwrap());
    if models.is_empty() && handles.is_empty() {
        return vec![];
    }
    // variables holding one instance of a model: `user = User.objects.get(..)`, `$u = new User`
    let instances: HashMap<&str, &str> = ASSIGN.captures_iter(code).filter(|c| models.contains_key(&c[2])).map(|c| (c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str())).collect();
    let mut out: Vec<String> = vec![];
    for m in WORD.find_iter(code) {
        let w = m.as_str();
        let prev = code[..m.start()].chars().next_back();
        let member = matches!(prev, Some('.')) || code[..m.start()].ends_with("->") || code[..m.start()].ends_with("::");
        let (methods, args) = chain(code, m.end());
        if methods.is_empty() {
            continue;
        }
        let verbs = |oi: usize| {
            let o = &ORMS[oi];
            let write = methods.iter().any(|x| o.writes.contains(x));
            let read = methods.iter().any(|x| o.reads.contains(x));
            (read, write)
        };
        let label = if let (Some(&oi), false) = (models.get(w), member) {
            let (read, write) = verbs(oi);
            (read || write).then(|| (write, w.to_string()))
        } else if let Some((_, oi)) = handles.iter().find(|(n, _)| n == w) {
            let (read, write) = verbs(*oi);
            let model = WORD.find_iter(&args).map(|x| x.as_str()).find(|x| models.contains_key(*x)).unwrap_or("database");
            (read || write).then(|| (write, model.to_string()))
        } else if let (Some(model), false) = (instances.get(w), member) {
            let (_, write) = verbs(models[*model]);
            write.then(|| (true, model.to_string()))
        } else {
            None
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
