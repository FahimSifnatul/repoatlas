//! Optional compiler-grade precision: a SCIP index (`index.scip`, written by `rust-analyzer scip .`,
//! `scip-python index`, `scip-typescript index`, `scip-java`, `scip-go` ...) says exactly which definition each
//! name in the code refers to. repoatlas joins it to the call sites it finds itself (see `graph.rs`): where the index
//! has an answer, that answer is used; everywhere else the rules of `graph.rs` still apply.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use protobuf::Message;
use scip::types::{Index, PositionEncoding};

/// Where a symbol is defined: the range of its name.
#[derive(Clone, Debug)]
pub struct DefLoc {
    pub path: u32,
    pub line: u32,
    pub col: u32,
    pub end_col: u32,
}

#[derive(Default)]
pub struct Scip {
    paths: HashMap<String, u32>,
    names: Vec<String>,
    /// per file: how columns are counted (UTF-8 bytes, UTF-16 or UTF-32 units)
    enc: Vec<PositionEncoding>,
    syms: Vec<String>,
    /// (file, line, column) where a name is written -> its symbol
    at: HashMap<(u32, u32, u32), u32>,
    /// symbol -> where it is defined (files of the repo only)
    defs: HashMap<u32, Vec<DefLoc>>,
    /// symbol -> symbols that implement it
    impls: HashMap<u32, Vec<u32>>,
}

/// Columns of `line` as the index counts them -> byte offsets, and back.
fn units_to_byte(line: &str, col: u32, enc: PositionEncoding) -> Option<usize> {
    match enc {
        PositionEncoding::UTF16CodeUnitOffsetFromLineStart => {
            let mut n = 0u32;
            for (i, ch) in line.char_indices() {
                if n >= col {
                    return Some(i);
                }
                n += ch.len_utf16() as u32;
            }
            (n >= col).then_some(line.len())
        }
        PositionEncoding::UTF32CodeUnitOffsetFromLineStart => line.char_indices().map(|x| x.0).chain([line.len()]).nth(col as usize),
        _ => (col as usize <= line.len()).then_some(col as usize),
    }
}

fn byte_to_units(line: &str, byte: usize, enc: PositionEncoding) -> u32 {
    let head = line.get(..byte).unwrap_or("");
    (match enc {
        PositionEncoding::UTF16CodeUnitOffsetFromLineStart => head.encode_utf16().count(),
        PositionEncoding::UTF32CodeUnitOffsetFromLineStart => head.chars().count(),
        _ => byte,
    }) as u32
}

impl Scip {
    /// Read an index. Only files in `known` (repo-relative paths) are kept.
    pub fn load(file: &Path, known: &HashSet<String>) -> Result<Scip, String> {
        let bytes = std::fs::read(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
        let index = Index::parse_from_bytes(&bytes).map_err(|e| format!("{} is not a SCIP index: {e}", file.display()))?;
        let mut s = Scip::default();
        let mut sym_ids: HashMap<String, u32> = HashMap::new();
        let mut intern = |s: &mut Scip, name: &str| -> u32 {
            *sym_ids.entry(name.to_string()).or_insert_with(|| {
                s.syms.push(name.to_string());
                (s.syms.len() - 1) as u32
            })
        };
        let mut rels: Vec<(String, String)> = vec![];
        for info in index.external_symbols.iter().chain(index.documents.iter().flat_map(|d| d.symbols.iter())) {
            for r in info.relationships.iter().filter(|r| r.is_implementation) {
                rels.push((r.symbol.clone(), info.symbol.clone()));
            }
        }
        for doc in &index.documents {
            let path = doc.relative_path.replace('\\', "/");
            if !known.contains(&path) {
                continue;
            }
            let pid = s.names.len() as u32;
            s.paths.insert(path.clone(), pid);
            s.names.push(path);
            let enc = doc.position_encoding.enum_value().unwrap_or(PositionEncoding::UnspecifiedPositionEncoding);
            s.enc.push(enc);
            for occ in &doc.occurrences {
                if occ.symbol.is_empty() {
                    continue;
                }
                let (line, col, end_col) = match occ.range.as_slice() {
                    [l, c, e] if *l >= 0 => (*l as u32, *c as u32, *e as u32),
                    [l, c, el, e] if *l >= 0 && l == el => (*l as u32, *c as u32, *e as u32),
                    _ => continue,
                };
                let sym = intern(&mut s, &occ.symbol);
                s.at.insert((pid, line, col), sym);
                if occ.symbol_roles & 1 != 0 {
                    s.defs.entry(sym).or_default().push(DefLoc { path: pid, line, col, end_col });
                }
            }
        }
        for (base, implementor) in rels {
            let (b, i) = (intern(&mut s, &base), intern(&mut s, &implementor));
            s.impls.entry(b).or_default().push(i);
        }
        Ok(s)
    }

    pub fn documents(&self) -> usize {
        self.names.len()
    }

    /// The symbol written at byte `byte` of `line` (the text of that line from its start) in `path`.
    pub fn symbol_at(&self, path: &str, line: u32, text: &str, byte: usize) -> Option<u32> {
        let p = *self.paths.get(path)?;
        let col = byte_to_units(text, byte, self.enc[p as usize]);
        self.at.get(&(p, line, col)).copied()
    }

    pub fn defs_of(&self, sym: u32) -> &[DefLoc] {
        self.defs.get(&sym).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn implementors(&self, sym: u32) -> &[u32] {
        self.impls.get(&sym).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn path_name(&self, p: u32) -> &str {
        &self.names[p as usize]
    }

    /// byte range of a definition's name in its line of text
    pub fn name_bytes(&self, d: &DefLoc, text: &str) -> Option<(usize, usize)> {
        let enc = self.enc[d.path as usize];
        Some((units_to_byte(text, d.col, enc)?, units_to_byte(text, d.end_col, enc)?))
    }
}
