//! install.sh: where the file lands, the PATH edit, downloading, checksums, failures, uninstall.
//!
//! Every test runs the real script with a throw-away $HOME. Downloads come from a small local server that
//! imitates the GitHub URLs, so nothing touches the network or your real shell startup files.
#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use tempfile::TempDir;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
const BASH_RC: &str = if cfg!(target_os = "macos") { ".bash_profile" } else { ".bashrc" };
const SKIP: &[&str] = &["__pycache__", "dist", "target", ".git", "build"];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}
fn install_sh() -> PathBuf {
    root().join("install.sh")
}
/// the release file install.sh picks for this machine
fn asset() -> String {
    format!("repoatlas-{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
fn cargo_dir() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).find(|d| d.join("cargo").is_file())
}
/// the installer builds from a clone, so those tests need cargo
macro_rules! need_cargo {
    () => {
        if cargo_dir().is_none() {
            eprintln!("skipped: needs Rust's cargo");
            return;
        }
    };
}

/// a copy of the project, standing in for `git clone`
fn copy_source(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap().flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if SKIP.contains(&name.as_ref()) || name.ends_with(".egg-info") {
            continue;
        }
        let to = dst.join(name.as_ref());
        if e.file_type().unwrap().is_dir() {
            copy_source(&e.path(), &to);
        } else {
            fs::copy(e.path(), &to).unwrap();
        }
    }
}

fn sha256(file: &Path) -> String {
    let out = Command::new("sha256sum").arg(file).output().or_else(|_| Command::new("shasum").args(["-a", "256"]).arg(file).output()).unwrap();
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap().to_string()
}

/// serves files under `dir` on a free local port
fn serve_dir(dir: PathBuf) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let dir = dir.clone();
            thread::spawn(move || answer(stream, &dir));
        }
    });
    port
}
fn answer(mut stream: TcpStream, dir: &Path) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) <= 2 {
            break;
        }
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/');
    let body = if path.contains("..") { None } else { fs::read(dir.join(path)).ok().filter(|_| dir.join(path).is_file()) };
    let _ = match body {
        Some(b) => {
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len());
            stream.write_all(&b)
        }
        None => stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    };
}

struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
}

#[derive(Default)]
struct Run<'a> {
    args: &'a [&'a str],
    script: Option<PathBuf>,
    /// feed the script on stdin, the way `curl | sh` does
    piped: bool,
    env: Vec<(&'a str, String)>,
}

/// a fresh fake home directory per test
struct Sandbox {
    tmp: TempDir,
    home: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::Builder::new().prefix("repoatlas-test-").tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir(&home).unwrap();
        Sandbox { tmp, home }
    }
    fn path(&self) -> &Path {
        self.tmp.path()
    }
    fn clone_dir(&self) -> PathBuf {
        let dir = self.path().join("clone");
        copy_source(root(), &dir);
        dir
    }
    fn base_path(&self) -> String {
        match cargo_dir() {
            Some(d) => format!("{}:{SYSTEM_PATH}", d.display()),
            None => SYSTEM_PATH.to_string(),
        }
    }
    fn run(&self, r: Run) -> Out {
        let real_home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| real_home.join(d).display().to_string());
        let mut cmd = Command::new("sh");
        match (&r.script, r.piped) {
            (_, true) => cmd.args(["-s", "--"]).args(r.args),
            (s, false) => cmd.arg(s.clone().unwrap_or_else(install_sh)).args(r.args),
        };
        // builds from a clone reuse one target folder and skip link-time optimisation, so they take seconds, not minutes
        cmd.env_clear()
            .env("HOME", &self.home)
            .env("PATH", self.base_path())
            .env("SHELL", "/bin/bash")
            .env("TMPDIR", self.path())
            .env("CARGO_TARGET_DIR", root().join("target").join("install-tests"))
            .env("CARGO_PROFILE_RELEASE_LTO", "false")
            .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "16")
            .env("RUSTUP_HOME", var("RUSTUP_HOME", ".rustup"))
            .env("CARGO_HOME", var("CARGO_HOME", ".cargo"));
        for (k, v) in &r.env {
            cmd.env(k, v);
        }
        cmd.current_dir(self.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let text = if r.piped { fs::read(install_sh()).unwrap() } else { vec![] };
        let feeder = thread::spawn(move || {
            let _ = stdin.write_all(&text);
        });
        let out = child.wait_with_output().unwrap();
        feeder.join().unwrap();
        Out { ok: out.status.success(), stdout: String::from_utf8_lossy(&out.stdout).into(), stderr: String::from_utf8_lossy(&out.stderr).into() }
    }
    fn install(&self, script: &Path) -> Out {
        self.run(Run { script: Some(script.to_path_buf()), ..Run::default() })
    }
    /// open a NEW interactive shell, the way a new terminal window would
    fn shell_finds_repoatlas(&self) -> String {
        let flags = if cfg!(target_os = "macos") { "-lic" } else { "-ic" }; // a new Terminal window is a login shell on macOS
        let out = Command::new("bash")
            .args([flags, "command -v repoatlas && repoatlas --version"])
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("SHELL", "/bin/bash")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into()
    }
    /// `curl | sh` against a local imitation of GitHub: returns the environment that points the script at it
    fn serve(&self, release: bool, tamper: bool) -> Vec<(&'static str, String)> {
        let web = self.path().join("web");
        let repo = web.join("octo").join("repoatlas");
        if release {
            let d = repo.join("releases/latest/download");
            fs::create_dir_all(&d).unwrap();
            let file = d.join(asset());
            fs::copy(env!("CARGO_BIN_EXE_repoatlas"), &file).unwrap();
            let sum = sha256(&file);
            if tamper {
                fs::OpenOptions::new().append(true).open(&file).unwrap().write_all(b"#tampered").unwrap();
            }
            fs::write(d.join(format!("{}.sha256", asset())), format!("{sum}  {}\n", asset())).unwrap();
        }
        let a = repo.join("archive/refs/heads");
        fs::create_dir_all(&a).unwrap();
        let stage = self.path().join("stage");
        copy_source(root(), &stage.join("repoatlas-master"));
        let tar = Command::new("tar").arg("czf").arg(a.join("master.tar.gz")).arg("-C").arg(&stage).arg("repoatlas-master").status().unwrap();
        assert!(tar.success());
        let port = serve_dir(web);
        vec![("REPOATLAS_BASE_URL", format!("http://127.0.0.1:{port}")), ("REPOATLAS_REPO", "octo/repoatlas".into())]
    }
}

fn piped<'a>(env: Vec<(&'a str, String)>) -> Run<'a> {
    Run { piped: true, env, ..Run::default() }
}

// ---- from a clone ----

#[test]
fn installs_to_path_and_survives_deleting_the_clone() {
    need_cargo!();
    let sb = Sandbox::new();
    let clone = sb.clone_dir();
    let r = sb.install(&clone.join("install.sh"));
    assert!(r.ok, "{}", r.stderr);
    let exe = sb.home.join(".local/bin/repoatlas");
    assert!(fs::metadata(&exe).unwrap().permissions().mode() & 0o111 != 0);
    assert!(r.stdout.contains("Added $HOME/.local/bin to your PATH"));
    assert!(r.stdout.contains("you can delete the folder"));

    fs::rename(&clone, sb.path().join("gone")).unwrap(); // the whole point: nothing depends on the clone
    let out = sb.shell_finds_repoatlas();
    assert!(out.contains(exe.to_str().unwrap()), "{out}");
    assert!(out.contains(&format!("repoatlas {VERSION}")), "{out}");

    let site = sb.path().join("site");
    fs::create_dir(&site).unwrap();
    fs::write(site.join("app.py"), "def hello():\n    pass\n").unwrap();
    let html = sb.path().join("out.html");
    let run = Command::new(&exe).arg(&site).arg("-o").arg(&html).arg("--no-open").output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(fs::read_to_string(html).unwrap().contains("architecture map"));
}

#[test]
fn running_it_twice_edits_startup_file_once() {
    need_cargo!();
    let sb = Sandbox::new();
    let clone = sb.clone_dir();
    for _ in 0..2 {
        assert!(sb.install(&clone.join("install.sh")).ok);
    }
    let rc = fs::read_to_string(sb.home.join(BASH_RC)).unwrap();
    assert_eq!(rc.matches("added by the repoatlas installer").count(), 1);
}

#[test]
fn startup_file_per_shell() {
    need_cargo!();
    let sb = Sandbox::new();
    let script = sb.clone_dir().join("install.sh");
    let with_shell = |shell: &str| sb.run(Run { script: Some(script.clone()), env: vec![("SHELL", shell.into())], ..Run::default() });
    with_shell("/usr/bin/zsh");
    assert!(fs::read_to_string(sb.home.join(".zshrc")).unwrap().contains(r#"export PATH="$HOME/.local/bin:$PATH""#));
    with_shell("/usr/bin/fish");
    assert!(fs::read_to_string(sb.home.join(".config/fish/conf.d/repoatlas.fish")).unwrap().contains("fish_add_path $HOME/.local/bin"));
    with_shell("/bin/dash");
    assert!(fs::read_to_string(sb.home.join(".profile")).unwrap().contains("added by the repoatlas installer"));
}

#[test]
fn bash_on_macos_uses_bash_profile() {
    need_cargo!();
    let sb = Sandbox::new();
    let fake = sb.path().join("fakebin");
    fs::create_dir(&fake).unwrap();
    fs::write(fake.join("uname"), "#!/bin/sh\necho Darwin\n").unwrap();
    fs::set_permissions(fake.join("uname"), fs::Permissions::from_mode(0o755)).unwrap();
    let script = sb.clone_dir().join("install.sh");
    sb.run(Run { script: Some(script), env: vec![("PATH", format!("{}:{}", fake.display(), sb.base_path()))], ..Run::default() });
    assert!(sb.home.join(".bash_profile").exists());
    assert!(!sb.home.join(".bashrc").exists());
}

#[test]
fn leaves_startup_files_alone_when_already_on_path() {
    need_cargo!();
    let sb = Sandbox::new();
    let script = sb.clone_dir().join("install.sh");
    let r = sb.run(Run { script: Some(script), env: vec![("PATH", format!("{}/.local/bin:{}", sb.home.display(), sb.base_path()))], ..Run::default() });
    assert!(r.ok, "{}", r.stderr);
    assert!(!sb.home.join(BASH_RC).exists());
    assert!(r.stdout.contains("Ready."));
}

#[test]
fn no_modify_path_and_custom_dir() {
    need_cargo!();
    let sb = Sandbox::new();
    let script = sb.clone_dir().join("install.sh");
    let target = sb.path().join("mybin");
    let dir = target.display().to_string();
    let r = sb.run(Run { args: &["--no-modify-path"], script: Some(script.clone()), env: vec![("REPOATLAS_INSTALL_DIR", dir.clone())], ..Run::default() });
    assert!(r.ok, "{}", r.stderr);
    assert!(target.join("repoatlas").exists());
    assert!(!sb.home.join(BASH_RC).exists());
    assert!(r.stdout.contains("is not on your PATH"));
    sb.run(Run { script: Some(script), env: vec![("REPOATLAS_INSTALL_DIR", dir.clone())], ..Run::default() });
    assert!(fs::read_to_string(sb.home.join(BASH_RC)).unwrap().contains(&format!(r#"export PATH="{dir}:$PATH""#)));
}

// ---- `curl | sh`: the script is fed on stdin and has no clone next to it ----

#[test]
fn installs_the_release_file() {
    need_cargo!();
    let sb = Sandbox::new();
    let r = sb.run(piped(sb.serve(true, false)));
    assert!(r.ok, "{}", r.stderr);
    assert!(r.stdout.contains("Downloading repoatlas"));
    assert!(!r.stdout.to_lowercase().contains("building from source"));
    assert!(sb.shell_finds_repoatlas().contains(&format!("repoatlas {VERSION}")));
}

#[test]
fn corrupted_download_is_rejected() {
    need_cargo!();
    let sb = Sandbox::new();
    let r = sb.run(piped(sb.serve(true, true)));
    assert!(!r.ok);
    assert!(r.stderr.contains("checksum mismatch"), "{}", r.stderr);
    assert!(!sb.home.join(".local/bin/repoatlas").exists());
}

#[test]
fn without_a_release_it_builds_from_source() {
    need_cargo!();
    let sb = Sandbox::new();
    let r = sb.run(piped(sb.serve(false, false)));
    assert!(r.ok, "{}", r.stderr);
    assert!(r.stdout.to_lowercase().contains("building from source"));
    assert!(sb.shell_finds_repoatlas().contains(&format!("repoatlas {VERSION}")));
}

#[test]
fn wrong_repository_is_a_clear_error() {
    need_cargo!();
    let sb = Sandbox::new();
    let mut env = sb.serve(true, false);
    env.retain(|(k, _)| *k != "REPOATLAS_REPO");
    env.push(("REPOATLAS_REPO", "nobody/nothing".into()));
    let r = sb.run(piped(env));
    assert!(!r.ok);
    assert!(r.stderr.contains("could not download"), "{}", r.stderr);
}

// ---- requirements ----

#[test]
fn building_without_cargo_says_how_to_get_it() {
    let sb = Sandbox::new();
    if SYSTEM_PATH.split(':').any(|d| Path::new(d).join("cargo").is_file()) {
        eprintln!("skipped: cargo is on the system PATH");
        return;
    }
    let script = sb.clone_dir().join("install.sh");
    let r = sb.run(Run { script: Some(script), env: vec![("PATH", SYSTEM_PATH.into())], ..Run::default() });
    assert!(!r.ok);
    assert!(r.stderr.contains("needs Rust's cargo"), "{}", r.stderr);
    assert!(r.stderr.contains("sh.rustup.rs"));
    assert!(!sb.home.join(".local/bin/repoatlas").exists());
}

#[test]
fn unknown_option_and_help() {
    let sb = Sandbox::new();
    assert!(!sb.run(Run { args: &["--bogus"], ..Run::default() }).ok);
    let h = sb.run(Run { args: &["--help"], piped: true, ..Run::default() }); // also works when piped
    assert!(h.ok);
    assert!(h.stdout.contains("--uninstall"));
}

// ---- uninstall ----

#[test]
fn removes_repoatlas_but_never_a_foreign_file() {
    need_cargo!();
    let sb = Sandbox::new();
    let script = sb.clone_dir().join("install.sh");
    sb.install(&script);
    let exe = sb.home.join(".local/bin/repoatlas");
    let uninstall = || sb.run(Run { args: &["--uninstall"], script: Some(script.clone()), ..Run::default() });
    let r = uninstall();
    assert!(r.ok, "{}", r.stderr);
    assert!(!exe.exists());

    fs::write(&exe, "#!/bin/sh\necho 'something else entirely'\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let r = uninstall();
    assert!(!r.ok);
    assert!(r.stderr.contains("leaving it alone"), "{}", r.stderr);
    assert!(exe.exists());
}

#[test]
fn uninstall_when_not_installed() {
    let sb = Sandbox::new();
    let r = sb.run(Run { args: &["--uninstall"], ..Run::default() });
    assert!(!r.ok);
    assert!(r.stderr.contains("not installed"), "{}", r.stderr);
}
