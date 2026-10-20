// dev helper: cargo run --example calls -- <repo> <file>   (every call in <file> and what it resolved to)
use repoatlas::{finder::Finder, graph, parse, resolve::Project};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let (mut files, man) = repoatlas::scan::scan(root);
    files.iter_mut().for_each(parse::parse);
    let project = Project::new(root, &files, man);
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let find = Finder::new(&paths);
    let refs = graph::link(&files, &find);
    for (i, f) in files.iter_mut().enumerate() {
        let (mut u, _) = project.resolve(f);
        u.extend(refs[i].iter().cloned());
        u.remove(&f.path);
        f.uses = u.into_iter().collect();
    }
    let t = graph::resolve_calls(&files);
    let fi = paths.iter().position(|p| p == &a[2]).expect("file");
    let f = &files[fi];
    println!("uses {:?}", f.uses);
    for (ci, c) in f.calls.iter().enumerate() {
        let to: Vec<String> = t[fi][ci].iter().map(|g| format!("{}:{}", files[g.0].path, files[g.0].defs[g.1].name)).collect();
        println!("{:>5} {:<20} recv={:<24} ref={} in={} -> {:?}", c.line, c.name, c.recv, c.is_ref, f.defs[c.def].name, to);
    }
}
