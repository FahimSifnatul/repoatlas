"""install.sh: where the file lands, the PATH edit, downloading, checksums, failures, uninstall.

Every test runs the real script with a throw-away $HOME. Downloads come from a small local server that
imitates the GitHub URLs, so nothing touches the network or your real shell startup files.
"""
import hashlib
import os
import platform
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
PYDIR = str(Path(sys.executable).parent)
PLACEHOLDER = "YOUR_GITHUB_" + "USER"        # split, so personalize.sh does not rewrite this test
SYSTEM_PATH = f"{PYDIR}:/usr/local/bin:/usr/bin:/bin"
# where the installer writes the PATH line for bash: macOS Terminal starts login shells, which read .bash_profile
BASH_RC = ".bash_profile" if platform.system() == "Darwin" else ".bashrc"


class Quiet(SimpleHTTPRequestHandler):
    def log_message(self, *a):
        pass


def build_binary():
    subprocess.run([sys.executable, str(ROOT / "scripts" / "build_binary.py")], check=True, capture_output=True)
    return ROOT / "dist" / "repoatlas"


class Sandbox(unittest.TestCase):
    """A fresh fake home directory per test."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="repoatlas-test-"))
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def env(self, **extra):
        e = {"HOME": str(self.home), "PATH": SYSTEM_PATH, "SHELL": "/bin/bash", "TMPDIR": str(self.tmp)}
        e.update(extra)
        return e

    def run_install(self, *args, stdin=None, script=INSTALL, **env):
        return subprocess.run(["sh", str(script), *args] if stdin is None else ["sh", "-s", "--", *args],
                              input=stdin, capture_output=True, text=True, env=self.env(**env), cwd=str(self.tmp))

    def clone(self):
        """A copy of the project, standing in for `git clone`."""
        dst = self.tmp / "clone"
        shutil.copytree(ROOT, dst, ignore=shutil.ignore_patterns("__pycache__", "dist", ".git", "build", "*.egg-info"))
        return dst

    def shell_finds_repoatlas(self, rc_shell="bash"):
        """Open a NEW interactive shell, the way a new terminal window would."""
        flags = "-lic" if platform.system() == "Darwin" else "-ic"        # a new Terminal window is a login shell on macOS
        r = subprocess.run([rc_shell, flags, "command -v repoatlas && repoatlas --version"], capture_output=True, text=True,
                           env={"HOME": str(self.home), "PATH": "/usr/bin:/bin", "SHELL": f"/bin/{rc_shell}"})
        return r.stdout


class FromClone(Sandbox):
    def test_installs_to_path_and_survives_deleting_the_clone(self):
        clone = self.clone()
        r = self.run_install(script=clone / "install.sh")
        self.assertEqual(r.returncode, 0, r.stderr)
        exe = self.home / ".local/bin/repoatlas"
        self.assertTrue(os.access(exe, os.X_OK))
        self.assertIn("Added $HOME/.local/bin to your PATH", r.stdout)
        self.assertIn("you can delete the folder", r.stdout)

        shutil.rmtree(clone)                                  # the whole point: nothing depends on the clone
        out = self.shell_finds_repoatlas()
        self.assertIn(str(exe), out)
        self.assertIn("repoatlas 1.0.0", out)

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
        self.run_install(script=clone / "install.sh", PATH=f"{fake}:{SYSTEM_PATH}")
        self.assertTrue((self.home / ".bash_profile").exists())
        self.assertFalse((self.home / ".bashrc").exists())

    def test_leaves_startup_files_alone_when_already_on_path(self):
        clone = self.clone()
        r = self.run_install(script=clone / "install.sh", PATH=f"{self.home}/.local/bin:{SYSTEM_PATH}")
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


class Download(Sandbox):
    """`curl | sh`: the script is fed on stdin and has no clone next to it."""

    @classmethod
    def setUpClass(cls):
        cls.binary = build_binary()

    def serve(self, release=True, tamper=False):
        web = self.tmp / "web"
        repo = web / "octo" / "repoatlas"
        if release:
            d = repo / "releases" / "latest" / "download"
            d.mkdir(parents=True)
            data = self.binary.read_bytes()
            (d / "repoatlas").write_bytes(data + (b"#tampered" if tamper else b""))
            (d / "repoatlas.sha256").write_text(f"{hashlib.sha256(data).hexdigest()}  repoatlas\n")
        a = repo / "archive" / "refs" / "heads"
        a.mkdir(parents=True)
        with tarfile.open(a / "master.tar.gz", "w:gz") as t:
            skip = {"__pycache__", "dist", ".git", "build"}
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
        self.assertIn("repoatlas 1.0.0", self.shell_finds_repoatlas())

    def test_corrupted_download_is_rejected(self):
        r = self.run_install(stdin=INSTALL.read_text(), **self.serve(tamper=True))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("checksum mismatch", r.stderr)
        self.assertFalse((self.home / ".local/bin/repoatlas").exists())

    def test_without_a_release_it_builds_from_source(self):
        r = self.run_install(stdin=INSTALL.read_text(), **self.serve(release=False))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("building from source", r.stdout.lower())
        self.assertIn("repoatlas 1.0.0", self.shell_finds_repoatlas())

    def test_wrong_repository_is_a_clear_error(self):
        env = self.serve()
        env["REPOATLAS_REPO"] = "nobody/nothing"
        r = self.run_install(stdin=INSTALL.read_text(), **env)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("could not download", r.stderr)


class Requirements(Sandbox):
    def test_no_python_says_how_to_get_it(self):
        r = subprocess.run(["/bin/sh", str(INSTALL)], capture_output=True, text=True, env={"HOME": str(self.home), "PATH": "/nonexistent"})
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Python 3 is required", r.stderr)
        self.assertIn("apt install python3", r.stderr)

    def test_old_python_is_refused(self):
        fake = self.tmp / "oldpy"
        fake.mkdir()
        (fake / "python3").write_text('#!/bin/sh\n[ "$1" = "-V" ] && echo "Python 3.7.9" && exit 0\nexit 1\n')
        (fake / "python3").chmod(0o755)
        r = self.run_install(PATH=f"{fake}:/usr/bin:/bin")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("3.8 or newer", r.stderr)
        self.assertIn("3.7.9", r.stderr)

    def test_unknown_option_and_help(self):
        self.assertNotEqual(self.run_install("--bogus").returncode, 0)
        h = self.run_install("--help", stdin=INSTALL.read_text())          # also works when piped
        self.assertEqual(h.returncode, 0)
        self.assertIn("--uninstall", h.stdout)


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



class Helpers(Sandbox):
    """personalize.sh / update_formula.sh, tested on a miniature template so the result does not depend on
    whether *this* repository has already been personalized."""

    def template(self):
        t = self.tmp / "template"
        (t / "scripts").mkdir(parents=True)
        (t / "packaging/homebrew/Formula").mkdir(parents=True)
        for name in ("personalize.sh", "update_formula.sh"):
            shutil.copy(ROOT / "scripts" / name, t / "scripts" / name)
        (t / "install.sh").write_text(f'REPO="{PLACEHOLDER}/repoatlas"\n')
        (t / "README.md").write_text(f"curl https://raw.githubusercontent.com/{PLACEHOLDER}/repoatlas/main/install.sh | sh\n")
        (t / "packaging/homebrew/Formula/repoatlas.rb").write_text(
            f'class Repoatlas < Formula\n  homepage "https://github.com/{PLACEHOLDER}/repoatlas"\n'
            '  url "https://example.invalid/old.tar.gz"\n  sha256 "REPLACE_WITH_SHA256"\nend\n')
        return t

    def test_personalize_replaces_every_placeholder_and_only_those(self):
        t = self.template()
        script_before = (t / "scripts/personalize.sh").read_text()
        r = subprocess.run(["sh", str(t / "scripts/personalize.sh"), "octocat"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("3 files", r.stdout)
        for f in ("install.sh", "README.md", "packaging/homebrew/Formula/repoatlas.rb"):
            text = (t / f).read_text()
            self.assertNotIn(PLACEHOLDER, text, f)
            self.assertIn("octocat/repoatlas", text, f)
        self.assertEqual((t / "scripts/personalize.sh").read_text(), script_before)     # did not rewrite itself
        bad = subprocess.run(["sh", str(t / "scripts/personalize.sh"), "bad name!"], capture_output=True, text=True)
        self.assertNotEqual(bad.returncode, 0)

    def test_update_formula_fills_url_and_checksum(self):
        t = self.template()
        subprocess.run(["sh", str(t / "scripts/personalize.sh"), "octocat"], check=True, capture_output=True)
        tarball = self.tmp / "v1.0.0.tar.gz"
        tarball.write_bytes(b"pretend this is the release tarball")
        r = subprocess.run(["sh", str(t / "scripts/update_formula.sh"), "v1.0.0"], capture_output=True, text=True,
                           env=self.env(REPOATLAS_TARBALL_URL=tarball.as_uri()))
        self.assertEqual(r.returncode, 0, r.stderr)
        formula = (t / "packaging/homebrew/Formula/repoatlas.rb").read_text()
        self.assertIn(f'sha256 "{hashlib.sha256(tarball.read_bytes()).hexdigest()}"', formula)
        self.assertIn(f'url "{tarball.as_uri()}"', formula)
        self.assertNotIn("REPLACE_WITH", formula)

    def test_update_formula_asks_to_personalize_first(self):
        t = self.template()
        r = subprocess.run(["sh", str(t / "scripts/update_formula.sh"), "v1.0.0"], capture_output=True, text=True, env=self.env())
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("personalize.sh", r.stderr)


if __name__ == "__main__":
    unittest.main()
