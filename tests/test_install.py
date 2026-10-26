"""install.sh: where the file lands, the PATH edit, downloading, checksums, failures, uninstall.

Every test runs the real script with a throw-away $HOME. Downloads come from a small local server that
imitates the GitHub URLs, so nothing touches the network or your real shell startup files.
"""
import hashlib
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import unittest
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INSTALL = ROOT / "install.sh"
VERSION = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)
SYSTEM_PATH = "/usr/local/bin:/usr/bin:/bin"
CARGO = shutil.which("cargo")
REAL_HOME = Path.home()
# the release file install.sh picks for this machine
ASSET = "repoatlas-{}-{}".format({"Darwin": "macos", "Linux": "linux"}.get(platform.system(), platform.system().lower()),
                                 {"AMD64": "x86_64", "amd64": "x86_64", "arm64": "aarch64"}.get(platform.machine(), platform.machine()))
# builds from a clone reuse one target folder and skip link-time optimisation, so they take seconds, not minutes
BUILD_ENV = {"CARGO_TARGET_DIR": str(ROOT / "target" / "install-tests"), "CARGO_PROFILE_RELEASE_LTO": "false",
             "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": "16",
             "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(REAL_HOME / ".rustup")),
             "CARGO_HOME": os.environ.get("CARGO_HOME", str(REAL_HOME / ".cargo"))}
# where the installer writes the PATH line for bash: macOS Terminal starts login shells, which read .bash_profile
BASH_RC = ".bash_profile" if platform.system() == "Darwin" else ".bashrc"


class Quiet(SimpleHTTPRequestHandler):
    def log_message(self, *a):
        pass


def build_binary():
    subprocess.run(["cargo", "build", "--release", "--locked", "--quiet"], cwd=ROOT, check=True, capture_output=True)
    return ROOT / "target" / "release" / "repoatlas"


def copy_source(dst):
    """A copy of the project, standing in for `git clone`."""
    shutil.copytree(ROOT, dst, ignore=shutil.ignore_patterns("__pycache__", "dist", "target", ".git", "build", "*.egg-info"))
    return dst


class Sandbox(unittest.TestCase):
    """A fresh fake home directory per test, and one copy of the project shared by the class (so it builds once)."""

    @classmethod
    def setUpClass(cls):
        cls.shared = Path(tempfile.mkdtemp(prefix="repoatlas-clone-"))
        copy_source(cls.shared / "clone")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.shared, ignore_errors=True)

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="repoatlas-test-"))
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def env(self, **extra):
        path = f"{Path(CARGO).parent}:{SYSTEM_PATH}" if CARGO else SYSTEM_PATH
        e = {"HOME": str(self.home), "PATH": path, "SHELL": "/bin/bash", "TMPDIR": str(self.tmp), **BUILD_ENV}
        e.update(extra)
        return e

    def run_install(self, *args, stdin=None, script=INSTALL, **env):
        return subprocess.run(["sh", str(script), *args] if stdin is None else ["sh", "-s", "--", *args],
                              input=stdin, capture_output=True, text=True, env=self.env(**env), cwd=str(self.tmp))

    def clone(self):
        return self.shared / "clone"

    def shell_finds_repoatlas(self, rc_shell="bash"):
        """Open a NEW interactive shell, the way a new terminal window would."""
        flags = "-lic" if platform.system() == "Darwin" else "-ic"        # a new Terminal window is a login shell on macOS
        r = subprocess.run([rc_shell, flags, "command -v repoatlas && repoatlas --version"], capture_output=True, text=True,
                           env={"HOME": str(self.home), "PATH": "/usr/bin:/bin", "SHELL": f"/bin/{rc_shell}"})
        return r.stdout


@unittest.skipUnless(CARGO, "needs Rust's cargo")
class FromClone(Sandbox):
    def test_installs_to_path_and_survives_deleting_the_clone(self):
        clone = self.clone()
        r = self.run_install(script=clone / "install.sh")
        self.assertEqual(r.returncode, 0, r.stderr)
        exe = self.home / ".local/bin/repoatlas"
        self.assertTrue(os.access(exe, os.X_OK))
        self.assertIn("Added $HOME/.local/bin to your PATH", r.stdout)
        self.assertIn("you can delete the folder", r.stdout)

        gone = self.tmp / "gone"                              # the whole point: nothing depends on the clone
        clone.rename(gone)
        self.addCleanup(gone.rename, clone)
        out = self.shell_finds_repoatlas()
        self.assertIn(str(exe), out)
        self.assertIn(f"repoatlas {VERSION}", out)

        site = self.tmp / "site"
        site.mkdir()
        (site / "app.py").write_text("def hello():\n    pass\n")
        run = subprocess.run([str(exe), str(site), "-o", str(self.tmp / "out.html"), "--no-open"], capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("architecture map", (self.tmp / "out.html").read_text())

    def test_running_it_twice_edits_startup_file_once(self):
        clone = self.clone()
        for _ in range(2):
            self.assertEqual(self.run_install(script=clone / "install.sh").returncode, 0)
        rc = (self.home / BASH_RC).read_text()
        self.assertEqual(rc.count("added by the repoatlas installer"), 1)

    def test_startup_file_per_shell(self):
        clone = self.clone()
        self.run_install(script=clone / "install.sh", SHELL="/usr/bin/zsh")
        self.assertIn('export PATH="$HOME/.local/bin:$PATH"', (self.home / ".zshrc").read_text())
        self.run_install(script=clone / "install.sh", SHELL="/usr/bin/fish")
        self.assertIn("fish_add_path $HOME/.local/bin", (self.home / ".config/fish/conf.d/repoatlas.fish").read_text())
        self.run_install(script=clone / "install.sh", SHELL="/bin/dash")
        self.assertIn("added by the repoatlas installer", (self.home / ".profile").read_text())

    def test_bash_on_macos_uses_bash_profile(self):
        fake = self.tmp / "fakebin"
        fake.mkdir()
        (fake / "uname").write_text("#!/bin/sh\necho Darwin\n")
        (fake / "uname").chmod(0o755)
        clone = self.clone()
        self.run_install(script=clone / "install.sh", PATH=f"{fake}:{self.env()['PATH']}")
        self.assertTrue((self.home / ".bash_profile").exists())
        self.assertFalse((self.home / ".bashrc").exists())

    def test_leaves_startup_files_alone_when_already_on_path(self):
        clone = self.clone()
        r = self.run_install(script=clone / "install.sh", PATH=f"{self.home}/.local/bin:{self.env()['PATH']}")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((self.home / BASH_RC).exists())
        self.assertIn("Ready.", r.stdout)

    def test_no_modify_path_and_custom_dir(self):
        clone = self.clone()
        target = self.tmp / "mybin"
        r = self.run_install("--no-modify-path", script=clone / "install.sh", REPOATLAS_INSTALL_DIR=str(target))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((target / "repoatlas").exists())
        self.assertFalse((self.home / BASH_RC).exists())
        self.assertIn("is not on your PATH", r.stdout)
        r = self.run_install(script=clone / "install.sh", REPOATLAS_INSTALL_DIR=str(target))
        self.assertIn(f'export PATH="{target}:$PATH"', (self.home / BASH_RC).read_text())


@unittest.skipUnless(CARGO, "needs Rust's cargo")
class Download(Sandbox):
    """`curl | sh`: the script is fed on stdin and has no clone next to it."""

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.binary = build_binary()

    def serve(self, release=True, tamper=False):
        web = self.tmp / "web"
        repo = web / "octo" / "repoatlas"
        if release:
            d = repo / "releases" / "latest" / "download"
            d.mkdir(parents=True)
            data = self.binary.read_bytes()
            (d / ASSET).write_bytes(data + (b"#tampered" if tamper else b""))
            (d / f"{ASSET}.sha256").write_text(f"{hashlib.sha256(data).hexdigest()}  {ASSET}\n")
        a = repo / "archive" / "refs" / "heads"
        a.mkdir(parents=True)
        with tarfile.open(a / "master.tar.gz", "w:gz") as t:
            skip = {"__pycache__", "dist", "target", ".git", "build"}
            t.add(ROOT, arcname="repoatlas-master", filter=lambda ti: None if (
                skip & set(ti.name.split("/")) or ti.name.endswith(".egg-info")) else ti)
        server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Quiet, directory=str(web)))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.shutdown)
        self.addCleanup(server.server_close)
        return dict(REPOATLAS_BASE_URL=f"http://127.0.0.1:{server.server_port}", REPOATLAS_REPO="octo/repoatlas")

    def test_installs_the_release_file(self):
        r = self.run_install(stdin=INSTALL.read_text(), **self.serve())
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("Downloading repoatlas", r.stdout)
        self.assertNotIn("building from source", r.stdout.lower())
        self.assertIn(f"repoatlas {VERSION}", self.shell_finds_repoatlas())

    def test_corrupted_download_is_rejected(self):
        r = self.run_install(stdin=INSTALL.read_text(), **self.serve(tamper=True))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("checksum mismatch", r.stderr)
        self.assertFalse((self.home / ".local/bin/repoatlas").exists())

    def test_without_a_release_it_builds_from_source(self):
        r = self.run_install(stdin=INSTALL.read_text(), **self.serve(release=False))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("building from source", r.stdout.lower())
        self.assertIn(f"repoatlas {VERSION}", self.shell_finds_repoatlas())

    def test_wrong_repository_is_a_clear_error(self):
        env = self.serve()
        env["REPOATLAS_REPO"] = "nobody/nothing"
        r = self.run_install(stdin=INSTALL.read_text(), **env)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("could not download", r.stderr)


class Requirements(Sandbox):
    def test_building_without_cargo_says_how_to_get_it(self):
        if shutil.which("cargo", path=SYSTEM_PATH):
            self.skipTest("cargo is on the system PATH")
        r = self.run_install(script=self.clone() / "install.sh", PATH=SYSTEM_PATH)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("needs Rust's cargo", r.stderr)
        self.assertIn("sh.rustup.rs", r.stderr)
        self.assertFalse((self.home / ".local/bin/repoatlas").exists())

    def test_unknown_option_and_help(self):
        self.assertNotEqual(self.run_install("--bogus").returncode, 0)
        h = self.run_install("--help", stdin=INSTALL.read_text())          # also works when piped
        self.assertEqual(h.returncode, 0)
        self.assertIn("--uninstall", h.stdout)


@unittest.skipUnless(CARGO, "needs Rust's cargo")
class Uninstall(Sandbox):
    def test_removes_repoatlas_but_never_a_foreign_file(self):
        clone = self.clone()
        self.run_install(script=clone / "install.sh")
        exe = self.home / ".local/bin/repoatlas"
        r = self.run_install("--uninstall", script=clone / "install.sh")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(exe.exists())

        exe.write_text("#!/bin/sh\necho 'something else entirely'\n")
        exe.chmod(0o755)
        r = self.run_install("--uninstall", script=clone / "install.sh")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("leaving it alone", r.stderr)
        self.assertTrue(exe.exists())

    def test_uninstall_when_not_installed(self):
        r = self.run_install("--uninstall")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("not installed", r.stderr)


if __name__ == "__main__":
    unittest.main()
