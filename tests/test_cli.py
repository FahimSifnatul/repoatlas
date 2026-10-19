"""The installed command: `repoatlas <repo>`, `python -m repoatlas`, --version, error messages."""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from repoatlas import __version__  # noqa: E402


def run(*args, cwd=None):
    return subprocess.run([sys.executable, "-m", "repoatlas", *args], cwd=cwd or ROOT, capture_output=True,
                          text=True, env={"PYTHONPATH": str(ROOT), "PATH": "/usr/bin:/bin"})


class Cli(unittest.TestCase):
    def test_version(self):
        r = run("--version")
        self.assertEqual((r.returncode, r.stdout.strip()), (0, f"repoatlas {__version__}"))

    def test_maps_a_folder(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "app").mkdir()
            (Path(d) / "app" / "main.py").write_text("def hello():\n    pass\n")
            out = Path(d) / "map.html"
            r = run(str(Path(d) / "app"), "-o", str(out), "--no-open")
            self.assertEqual(r.returncode, 0, r.stderr)
            html = out.read_text()
            self.assertIn("architecture map", html)
            self.assertIn('"hello"', html)
            self.assertNotIn("__TITLE__", html)

    def test_defaults_to_current_folder(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "a.py").write_text("x = 1\n")
            r = run("--no-open", cwd=d)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertTrue((Path(d) / "repoatlas.html").exists())

    def test_bad_folder_is_a_clear_error(self):
        r = run("/definitely/not/here", "--no-open")
        self.assertEqual(r.returncode, 1)
        self.assertIn("is not a folder", r.stderr)

    def test_empty_folder_is_a_clear_error(self):
        with tempfile.TemporaryDirectory() as d:
            r = run(d, "--no-open")
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("no source files", r.stderr)


if __name__ == "__main__":
    unittest.main()
