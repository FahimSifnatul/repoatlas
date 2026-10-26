//! A program's journey: from the definition it starts at, every call it makes in the order the code makes
//! them, down to where its input comes in and its output goes out. One more journey per web endpoint.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use serde_json::{json, Value};

use crate::graph::Gid;
use crate::parse::{File, Kind};
use crate::resolve::{basename, dirname};

pub const MAX_STEPS: usize = 400;
pub const MAX_ROUTE_STEPS: usize = 80;
pub const MAX_ROUTES: usize = 40;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug)]
pub struct Step {
    /// the file the step's code is in
    pub file: usize,
    pub def: Option<usize>,
    pub label: String,
    pub line: usize,
    pub depth: usize,
    /// start, call, dispatch, ref, recursion, loads, more
    pub kind: &'static str,
    /// shown earlier as this step
    pub seen: Option<usize>,
    /// calls not shown because the journey goes too deep or too long
    pub more: usize,
    /// the call that leads here was matched from a name, not proven
    pub guess: bool,
    /// what that call sits under in its caller: "if", "loop", "error" or ""
    pub when: &'static str,
}

pub struct Ctx<'a> {
    pub files: &'a [File],
    /// per file, per call: the definitions it runs
    pub targets: &'a [Vec<Vec<Gid>>],
    /// per file, per call: whether the match is only a guess
    pub guess: &'a [Vec<bool>],
    pub ix: &'a HashMap<String, usize>,
}

impl<'a> Ctx<'a> {
    pub fn label(&self, g: Gid) -> String {
        let d = &self.files[g.0].defs[g.1];
        match d.kind {
            Kind::Module => {
                // two main.py files: say which, api/main.py
                let p = &self.files[g.0].path;
                let b = basename(p);
                if self.files.iter().filter(|f| basename(&f.path) == b).count() > 1 {
                    p.rsplitn(3, '/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/")
                } else {
                    b.to_string()
                }
            }
            Kind::Main => d.name.clone(),
            Kind::Class | Kind::Type | Kind::Impl => d.name.clone(),
            Kind::Func => match &d.owner {
                Some(o) => format!("{o}.{}()", d.name),
                None => format!("{}()", d.name),
            },
        }
    }

    /// what the body of g calls, in source order: (call index, targets)
    fn calls_of(&self, g: Gid) -> Vec<(usize, &'a Vec<Gid>)> {
        let f = &self.files[g.0];
        let mut seen: BTreeSet<&Vec<Gid>> = BTreeSet::new();
        // calls inside a route registration belong to that route, not to the code that registers it
        let in_route = |at: usize| f.routes.iter().any(|r| r.def == g.1 && at > r.at && at < r.end);
        f.calls
            .iter()
            .enumerate()
            .filter(|(i, c)| c.def == g.1 && !self.targets[g.0][*i].is_empty() && (c.is_ref || !in_route(c.at)))
            .filter(|(i, _)| seen.insert(&self.targets[g.0][*i])) // calling the same thing twice is one step
            .filter(|(i, _)| self.targets[g.0][*i] != vec![g] || !f.calls[*i].is_ref)
            .map(|(i, _)| (i, &self.targets[g.0][i]))
            .collect()
    }

    /// the public functions a file defines, for a start that calls nothing itself (a library, a page)
    fn defines(&self, file: usize) -> Vec<Gid> {
        let f = &self.files[file];
        f.defs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == Kind::Func && d.parent == Some(0) && !d.name.starts_with('_'))
            .map(|(i, _)| (file, i))
            .take(12)
            .collect()
    }
}

/// Every definition the starts can run, and how often one file calls into another. Unlike `Walk` it has no depth
/// or step limit and shows each definition once: it is what the whole-codebase map is drawn from.
pub struct Reach {
    pub defs: BTreeSet<Gid>,
    /// (calling file, called file) -> calls
    pub files: BTreeMap<(usize, usize), usize>,
}

pub fn reach(ctx: &Ctx, starts: &[Gid]) -> Reach {
    let mut out = Reach { defs: BTreeSet::new(), files: BTreeMap::new() };
    // per file, per definition: its calls (built when a file is first reached)
    let mut by_def: HashMap<usize, HashMap<usize, Vec<usize>>> = HashMap::new();
    let mut todo: VecDeque<Gid> = VecDeque::new();
    // code that runs when a file loads: its top level, and for Go the whole package (var initializers, init())
    let mut loaded: BTreeSet<usize> = BTreeSet::new();
    let mut go_dirs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, f) in ctx.files.iter().enumerate().filter(|(_, f)| f.lang.name == "Go") {
        go_dirs.entry(dirname(&f.path)).or_default().push(i);
    }
    let mut does: HashMap<usize, bool> = HashMap::new();
    for &g in starts {
        if out.defs.insert(g) {
            todo.push_back(g);
        }
    }
    while let Some(g) = todo.pop_front() {
        let f = &ctx.files[g.0];
        let calls = by_def.entry(g.0).or_insert_with(|| {
            let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
            for (i, c) in f.calls.iter().enumerate() {
                m.entry(c.def).or_default().push(i);
            }
            m
        });
        let mut next: Vec<Gid> = vec![];
        if loaded.insert(g.0) {
            if g.1 != 0 {
                next.push((g.0, 0));
            }
            if f.lang.name == "Go" {
                for &q in go_dirs.get(dirname(&f.path)).into_iter().flatten() {
                    next.push((q, 0));
                    next.extend(ctx.files[q].defs.iter().enumerate().filter(|(_, d)| d.kind == Kind::Func && d.owner.is_none() && d.name == "init").map(|(i, _)| (q, i)));
                }
            }
            // an import runs the top level of the file it names, when that does anything
            for q in f.uses.iter().filter_map(|u| ctx.ix.get(u).copied()).filter(|&q| q != g.0 && ctx.files[q].layer != Some("tests")) {
                let acts = *does.entry(q).or_insert_with(|| {
                    let tg = &ctx.targets[q];
                    ctx.files[q].calls.iter().enumerate().any(|(i, c)| c.def == 0 && !tg[i].is_empty())
                });
                if acts {
                    next.push((q, 0));
                }
            }
        }
        let own = calls.get(&g.1).map(|v| v.as_slice()).unwrap_or(&[]);
        for &ci in own {
            next.extend(ctx.targets[g.0][ci].iter().copied());
        }
        // a start that calls nothing itself (a library, a page): what it defines and the scripts it loads
        if own.iter().all(|&ci| ctx.targets[g.0][ci].is_empty()) && f.defs[g.1].kind == Kind::Module {
            next.extend(ctx.defines(g.0));
            if f.lang.grammar.is_none() {
                next.extend(f.uses.iter().filter_map(|u| ctx.ix.get(u).filter(|&&i| ctx.files[i].lang.grammar.is_some()).map(|&i| (i, 0))));
            }
        }
        for t in next {
            if t.0 != g.0 {
                *out.files.entry((g.0, t.0)).or_default() += 1;
            }
            if out.defs.insert(t) {
                todo.push_back(t);
            }
        }
    }
    out
}

pub struct Walk<'c, 'a> {
    ctx: &'c Ctx<'a>,
    pub steps: Vec<Step>,
    shown: HashMap<Gid, usize>,
    stack: Vec<Gid>,
    limit: usize,
}

impl<'c, 'a> Walk<'c, 'a> {
    pub fn new(ctx: &'c Ctx<'a>, limit: usize) -> Self {
        Walk { ctx, steps: vec![], shown: HashMap::new(), stack: vec![], limit }
    }

    fn push(&mut self, s: Step) -> bool {
        if self.steps.len() >= self.limit {
            if let Some(last) = self.steps.last_mut().filter(|l| l.kind == "more") {
                last.more += 1;
            } else {
                let file = s.file;
                self.steps.push(Step { file, def: None, label: String::new(), line: 0, depth: 0, kind: "more", seen: None, more: 1, guess: false, when: "" });
            }
            return false;
        }
        self.steps.push(s);
        true
    }

    /// One step for g, then the steps for what it calls.
    pub fn visit(&mut self, g: Gid, depth: usize, kind: &'static str, guess: bool, when: &'static str) {
        let ctx = self.ctx;
        let d = &ctx.files[g.0].defs[g.1];
        if let Some(&i) = self.shown.get(&g) {
            let kind = if self.stack.contains(&g) { "recursion" } else { kind };
            self.push(Step { file: g.0, def: Some(g.1), label: ctx.label(g), line: d.line, depth, kind, seen: Some(i), more: 0, guess, when });
            return;
        }
        let at = self.steps.len();
        if !self.push(Step { file: g.0, def: Some(g.1), label: ctx.label(g), line: d.line, depth, kind, seen: None, more: 0, guess, when }) {
            return;
        }
        self.shown.insert(g, at);
        let mut calls = ctx.calls_of(g);
        let defines = calls.is_empty() && kind == "start" && d.kind == Kind::Module;
        if depth >= MAX_DEPTH {
            self.steps[at].more = calls.len();
            return;
        }
        self.stack.push(g);
        if defines {
            for h in ctx.defines(g.0) {
                self.visit(h, depth + 1, "defines", false, "");
            }
            // a page: the scripts it loads
            if ctx.files[g.0].lang.grammar.is_none() {
                for u in &ctx.files[g.0].uses {
                    if let Some(&i) = ctx.ix.get(u).filter(|&&i| ctx.files[i].lang.grammar.is_some()) {
                        self.visit((i, 0), depth + 1, "loads", false, "");
                    }
                }
            }
        }
        for (ci, targets) in calls.drain(..) {
            let c = &ctx.files[g.0].calls[ci];
            let kind = if c.is_ref { "ref" } else { "call" };
            if targets.len() == 1 {
                self.visit(targets[0], depth + 1, kind, ctx.guess[g.0][ci], c.cond);
            } else {
                // x.parse() on an object that can be any of several classes
                let label = format!("{}() — one of {}", c.name, targets.len());
                if !self.push(Step { file: g.0, def: None, label, line: c.line, depth: depth + 1, kind: "dispatch", seen: None, more: 0, guess: true, when: "" }) {
                    break;
                }
                for t in targets {
                    self.visit(*t, depth + 2, "call", ctx.guess[g.0][ci], c.cond);
                }
            }
        }
        self.stack.pop();
    }

    /// a module run before the code of a request (`auth.optional` handed to a route): one step, no calls
    pub fn middleware(&mut self, g: Gid) {
        let ctx = self.ctx;
        let d = &ctx.files[g.0].defs[g.1];
        self.push(Step { file: g.0, def: Some(g.1), label: ctx.label(g), line: d.line, depth: 0, kind: "middleware", seen: None, more: 0, guess: false, when: "" });
    }

    /// a file the framework loads by convention: one step, no calls
    pub fn loads(&mut self, file: usize) {
        let label = basename(&self.ctx.files[file].path).to_string();
        self.push(Step { file, def: Some(0), label, line: 1, depth: 0, kind: "loads", seen: None, more: 0, guess: false, when: "" });
    }
}

/// What a step takes in and gives out: its own body, and for the step a program starts at, the top of its file
/// too (`use clap::Parser`, `import argparse`: the file is loaded to run it).
pub fn step_io(ctx: &Ctx, s: &Step) -> Vec<&'static str> {
    let Some(d) = s.def.filter(|_| s.seen.is_none()) else { return vec![] };
    let defs = &ctx.files[s.file].defs;
    let mut io = defs[d].io.clone();
    if s.kind == "start" && d != 0 {
        io.extend(defs[0].io.iter().filter(|k| !io.contains(k)).copied().collect::<Vec<_>>());
        io.sort_by_key(|k| crate::tables::io_order(k));
    }
    io
}

pub fn steps_json(ctx: &Ctx, steps: &[Step]) -> Vec<Value> {
    steps
        .iter()
        .map(|s| {
            let f = &ctx.files[s.file];
            let d = s.def.map(|i| &f.defs[i]);
            let doc = match d {
                Some(d) if d.kind == Kind::Module => f.doc.clone(),
                Some(d) => d.doc.clone(),
                None => String::new(),
            };
            let io = step_io(ctx, s);
            let mut o = json!({"f": f.path, "n": s.label, "line": s.line, "d": s.depth, "k": s.kind});
            let m = o.as_object_mut().unwrap();
            if !doc.is_empty() && s.seen.is_none() {
                m.insert("doc".into(), json!(doc));
            }
            if !io.is_empty() {
                m.insert("io".into(), json!(io));
            }
            if let (Some(d), None) = (d, s.seen) {
                if !d.db.is_empty() {
                    m.insert("db".into(), json!(d.db));
                }
            }
            if let Some(i) = s.seen {
                m.insert("seen".into(), json!(i));
            }
            if s.more > 0 {
                m.insert("more".into(), json!(s.more));
            }
            if s.guess {
                m.insert("guess".into(), json!(true));
            }
            if !s.when.is_empty() {
                m.insert("when".into(), json!(s.when));
            }
            o
        })
        .collect()
}
