//! Command line: `repoatlas <repo>`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;

#[derive(Parser)]
#[command(name = "repoatlas", version,
          about = "Turn a code repository into an interactive, layer-by-layer architecture map (one HTML file).",
          after_help = "examples:
  repoatlas                                  map the current folder
  repoatlas ~/code/my-app                    map a local repository
  repoatlas https://github.com/pallets/flask map a repository straight from GitHub
  repoatlas . -o map.html --no-open          choose the output file, do not open the browser")]
struct Args {
    /// local folder or git URL (default: current folder)
    #[arg(default_value = ".")]
    repo: String,
    /// output file
    #[arg(short, long, default_value = "repoatlas.html")]
    out: PathBuf,
    /// do not open the result in a browser
    #[arg(long)]
    no_open: bool,
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("repoatlas: {msg}");
    ExitCode::from(1)
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("git@") || s.starts_with("ssh://")
}

fn expand(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(p),
    }
}

fn open(path: &Path) {
    let p = path.as_os_str();
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(p);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]).arg(p);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(p);
        c
    };
    let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

fn main() -> ExitCode {
    let a = Args::parse();
    let out = expand(&a.out.to_string_lossy());
    let out = if out.is_absolute() { out } else { std::env::current_dir().unwrap_or_default().join(out) };
    let result = if is_url(&a.repo) {
        let trimmed = a.repo.trim_end_matches('/');
        let name = trimmed.strip_suffix(".git").unwrap_or(trimmed).rsplit(['/', ':']).next().unwrap_or("repo").to_string();
        // the clone is removed afterwards
        let Ok(tmp) = tempfile::Builder::new().prefix("repoatlas-").tempdir() else { return fail("could not make a temporary folder") };
        eprintln!("Cloning…");
        match Command::new("git").args(["clone", "--depth", "1", "--quiet", &a.repo]).arg(tmp.path()).status() {
            Err(_) => return fail("git is not installed, so repoatlas cannot clone a URL. Clone it yourself and pass the folder."),
            Ok(s) if !s.success() => return fail(&format!("could not clone {}", a.repo)),
            Ok(_) => repoatlas::analyze(tmp.path(), &name),
        }
    } else {
        let root = expand(&a.repo);
        let Ok(root) = root.canonicalize().map_err(|_| ()).and_then(|r| if r.is_dir() { Ok(r) } else { Err(()) }) else {
            return fail(&format!("'{}' is not a folder", a.repo));
        };
        let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
        repoatlas::analyze(&root, &name)
    };
    let data = match result {
        Ok(d) => d,
        Err(e) => return fail(&e),
    };
    let r = &data["repo"];
    if let Err(e) = std::fs::write(&out, repoatlas::render(&data, r["name"].as_str().unwrap_or(""))) {
        return fail(&format!("could not write {}: {e}", out.display()));
    }
    let loc = repoatlas::text::thousands(r["loc"].as_u64().unwrap_or(0) as usize);
    println!("Scanned {} files ({loc} lines): {} layers, {} parts.\nWrote {}", r["files"], data["layers"].as_array().map_or(0, |l| l.len()), r["parts"], out.display());
    if !a.no_open {
        open(&out);
    }
    ExitCode::SUCCESS
}
