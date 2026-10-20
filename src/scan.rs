//! Walk the repository: source files with their text, plus the manifests, launch files, READMEs and
//! dependency files the later steps read.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::lang::{self, Lang};
use crate::parse::File;
use crate::tables::{DEP_FILES, LAUNCH_FILES, MAX_BYTES, SKIP_DIRS};

/// The language of a script with no extension (artisan, bin/www, manage), from its #! line.
fn shebang(path: &Path) -> Option<&'static Lang> {
    static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#!\s*(\S+)(?:\s+(?:-\S+\s+)*(\S+))?").unwrap());
    static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\d.]+$").unwrap());
    let mut buf = [0u8; 200];
    let n = fs::File::open(path).and_then(|mut f| f.read(&mut buf)).ok()?;
    let line = String::from_utf8_lossy(&buf[..n]);
    let line = line.lines().next()?;
    let m = LINE.captures(line)?;
    let mut exe = m[1].rsplit('/').next().unwrap_or("").to_string();
    if exe == "env" {
        if let Some(e) = m.get(2) {
            exe = e.as_str().to_string(); // #!/usr/bin/env php
        }
    }
    lang::by_interpreter(&VERSION.replace(&exe, "")).or_else(|| lang::by_interpreter(&exe))
}

pub fn scan(root: &Path) -> (Vec<File>, BTreeMap<String, Vec<String>>) {
    let mut files = vec![];
    let mut man: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let top = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| {
            !e.file_type().is_some_and(|t| t.is_dir())
                || e.depth() == 0
                || !(SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()) || e.path().join("pyvenv.cfg").exists())
        })
        .build();
    for entry in walker.flatten() {
        let p = entry.path();
        let Some(ft) = entry.file_type() else { continue };
        // never read a pipe / device, or follow a symlink out of the repo (it could point at ~/.aws or /proc)
        if ft.is_symlink() {
            match fs::canonicalize(p) {
                Ok(real) if real.starts_with(&top) && real.is_file() => {}
                _ => continue,
            }
        } else if !ft.is_file() {
            continue;
        }
        let Ok(rel) = p.strip_prefix(root) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let fname = entry.file_name().to_string_lossy().to_string();
        let size = fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX);
        if lang::is_manifest(&fname) {
            man.entry(fname.clone()).or_default().push(rel.clone());
        }
        if LAUNCH_FILES.iter().any(|(re, _)| re.is_match(&fname)) && size <= MAX_BYTES {
            man.entry("RUN".into()).or_default().push(rel.clone());
        }
        if fname.to_lowercase().starts_with("readme") {
            man.entry("README".into()).or_default().push(rel.clone());
        }
        if DEP_FILES.is_match(&fname) {
            man.entry("DEPS".into()).or_default().push(rel.clone());
        }
        let ext = fname.rfind('.').map(|i| fname[i..].to_lowercase());
        let plugin = match &ext {
            Some(e) => lang::by_ext(e),
            None => shebang(p),
        };
        let Some(plugin) = plugin else { continue };
        if fname.contains(".min.") || plugin.skip_suffixes.iter().any(|s| fname.ends_with(s)) || size > MAX_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(p) else { continue };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let loc = text.lines().filter(|l| !l.trim().is_empty()).count();
        if loc > 0 {
            files.push(File::new(rel, plugin, text, loc));
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    (files, man)
}
