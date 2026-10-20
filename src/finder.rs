//! Repo files by path (templates/page.html), by module name (food.settings, com.acme.App), by a name built at
//! run time ('html/ex%d.html') or by a command line (gunicorn app.wsgi:app).

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use crate::resolve::{dirname, join, normpath};
use crate::tables::{MODULE, PLACEHOLDER, STEM};

pub type Index = HashMap<String, Vec<String>>;

/// 'a/b/c.py' is findable as 'c.py', 'b/c.py' and 'a/b/c.py'.
pub fn suffix_index<'a>(keys: impl Iterator<Item = &'a String>) -> Index {
    let mut idx: Index = HashMap::new();
    for k in keys {
        let parts: Vec<&str> = k.split('/').collect();
        for i in 0..parts.len() {
            idx.entry(parts[i..].join("/")).or_default().push(k.clone());
        }
    }
    for v in idx.values_mut() {
        v.sort();
    }
    idx
}

/// a path inside folder d -> the repo path; "" for the root
pub fn repo_path(d: &str, rel: &str) -> String {
    let p = normpath(&join(d, rel));
    if p == "." { String::new() } else { p }
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
}

/// several candidates: keep the ones nearest folder d
fn nearest(hits: Vec<String>, d: &str) -> Vec<String> {
    if hits.len() < 2 {
        return hits;
    }
    let best = hits.iter().map(|h| common_prefix(d, h)).max().unwrap_or(0);
    hits.into_iter().filter(|h| common_prefix(d, h) == best).collect()
}

/// A path written in a file inside folder d -> the repo file it names (None when unknown or ambiguous).
pub fn find_path(tok: &str, d: &str, idx: &Index) -> Option<String> {
    static LEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\.\./|/)+").unwrap());
    let rel = normpath(&join(d, tok));
    if idx.get(&rel).is_some_and(|v| v.contains(&rel)) {
        return Some(rel);
    }
    let mut s = LEAD.replace(&normpath(tok), "").to_string();
    if tok.starts_with('/') && !s.contains('/') {
        return None; // "/__main__.py" is a piece of a path being built, not a path
    }
    let mut hits = idx.get(&s).cloned().unwrap_or_default();
    while hits.is_empty() && tok.starts_with('/') && s.contains('/') {
        // /app/server.py inside a container -> server.py
        s = s.split_once('/').unwrap().1.to_string();
        hits = idx.get(&s).cloned().unwrap_or_default();
    }
    let hits = nearest(hits, d);
    if hits.len() == 1 { hits.into_iter().next() } else { None }
}

pub struct Finder {
    pub files: Vec<String>,
    pub paths: Index,
    /// a/b.py, a/b/__init__.py, a/b/index.js, a/b/mod.rs -> a/b (only when one file has that stem)
    pub stem: BTreeMap<String, String>,
    pub stems: Index,
}

impl Finder {
    pub fn new(files: &[String]) -> Finder {
        static EXTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\.\w+)+$").unwrap());
        let mut stems: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for p in files {
            // and welcome.blade.php -> welcome
            let a = STEM.replace(p, "").to_string();
            let b = EXTS.replace(p, "").to_string();
            stems.entry(a.clone()).or_default().push(p.clone());
            if b != a {
                stems.entry(b).or_default().push(p.clone());
            }
        }
        let stem: BTreeMap<String, String> =
            stems.into_iter().filter(|(_, ps)| ps.len() == 1).map(|(s, ps)| (s, ps[0].clone())).collect();
        let mut sorted = files.to_vec();
        sorted.sort();
        Finder { paths: suffix_index(sorted.iter()), stems: suffix_index(stem.keys()), stem, files: sorted }
    }

    pub fn path(&self, tok: &str, d: &str) -> Option<String> {
        static HAS_EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\w+$").unwrap());
        let h = find_path(tok, d, &self.paths);
        // '/api/v1/users/' is a web address, not a file: only a relative path may leave out the extension
        if h.is_none() && tok.contains('/') && !tok.starts_with('/') && !HAS_EXT.is_match(tok) {
            // '../config/application' -> config/application.rb
            return find_path(tok, d, &self.stems).and_then(|h| self.stem.get(&h).cloned());
        }
        h
    }

    /// 'a.b.C' -> the file of module a/b/C, else a/b ('order.Order' is a model name, not the order package).
    /// bare: d is the folder a command runs in, and only a module right there counts (celery -A proj).
    pub fn module(&self, name: &str, d: &str, bare: bool) -> Option<String> {
        let parts: Vec<&str> = name.split('.').collect();
        let low = if bare { 0 } else { 1 };
        let mut k = parts.len();
        while k > low {
            let m = parts[..k].join("/");
            let h = if bare {
                [join(d, &format!("{m}/__main__")), join(d, &m)].into_iter().find(|s| self.stem.contains_key(s))
            } else {
                find_path(&format!("{m}/__main__"), d, &self.stems).or_else(|| find_path(&m, d, &self.stems))
            };
            if let Some(h) = h {
                return self.stem.get(&h).cloned();
            }
            k -= 1;
        }
        None
    }

    /// A file name built at run time ('html/ex%d.html', f"pages/{name}.html", 'sql/*.sql') -> the files it can be.
    pub fn pattern(&self, s: &str, d: &str) -> Vec<String> {
        static LEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\./|/)+").unwrap());
        static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[A-Za-z]\w{0,7}$").unwrap());
        static ANCHOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/|\w\w").unwrap());
        let s2 = LEAD.replace(s, "");
        let pieces: Vec<&str> = PLACEHOLDER.split(&s2).collect();
        if pieces.len() < 2 || !EXT.is_match(s) {
            return vec![];
        }
        let last = pieces[pieces.len() - 1];
        let last_stem = last.rsplit_once('.').map(|x| x.0).unwrap_or(last);
        if !ANCHOR.is_match(&format!("{}{}", pieces[0], last_stem)) {
            return vec![]; // '%s.py' alone could be any file
        }
        let body: Vec<String> = pieces.iter().map(|x| regex::escape(x)).collect();
        let Ok(re) = Regex::new(&format!("(?:^|/){}$", body.join("[^/]+"))) else { return vec![] };
        let hits: Vec<String> = self.files.iter().filter(|p| re.is_match(p)).cloned().collect();
        // the same pattern in several folders: keep the ones nearest the file that built it
        let mut hits = nearest(hits, d);
        hits.truncate(20);
        hits
    }

    /// Repo files a command line names: a path (node src/app.js, /app/entrypoint) or a module (python -m a.b,
    /// gunicorn a.wsgi:app). bare: d is the folder the command runs in, so a plain word is looked up there too.
    /// where_: a path inside a container -> the repo path that was copied there.
    pub fn command(&self, cmd: &str, d: &str, bare: bool, where_: Option<&dyn Fn(&str) -> Option<String>>) -> Vec<String> {
        static REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s\d?>>?\s*").unwrap());
        static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[^\s"'`\[\](){},;|&<>=]+"#).unwrap());
        static PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_]\w*$").unwrap());
        let head = REDIRECT.split(cmd).next().unwrap_or("");
        let mut out = vec![];
        for m in WORD.find_iter(head) {
            let mut w = m.as_str().to_string();
            if w.starts_with('-') || w.starts_with('$') {
                continue;
            }
            let mut here = None;
            if let (Some(f), true) = (where_, w.starts_with('/')) {
                here = Some(f(&w)); // a path inside the container: the exact repo file copied there
                if let Some(Some(h)) = &here {
                    w = h.clone();
                }
            }
            let module = w.split(':').next().unwrap_or("").to_string();
            let exact = match &here {
                Some(h) => h.clone(),
                None => Some(repo_path(d, &w)),
            };
            let hit = exact
                .filter(|e| self.paths.get(e).is_some_and(|v| v.contains(e))) // artisan, bin/www: a file named exactly
                .or_else(|| if w.contains(['/', '.']) { self.path(&w, d) } else { None })
                .or_else(|| if MODULE.is_match(&module) { self.module(&module, d, false) } else { None })
                .or_else(|| if bare && PLAIN.is_match(&w) { self.module(&w, d, true) } else { None });
            if let Some(h) = hit {
                out.push(h);
            }
        }
        out
    }
}

pub fn dir_of(p: &str) -> String {
    dirname(p).to_string()
}
