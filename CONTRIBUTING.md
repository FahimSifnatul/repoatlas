# Contributing

## Setup

There is nothing to install: repoatlas uses only the Python standard library.

```sh
git clone https://github.com/YOUR_GITHUB_USER/repoatlas.git && cd repoatlas
python3 -m repoatlas .                          # run it on itself
python3 -m unittest discover -s tests -v      # run the tests (Python 3.8 to 3.14)
shellcheck -s sh install.sh scripts/*.sh      # lint the shell scripts, if you touched them
```

## Layout

```
repoatlas/                    the Python package (what gets installed)
  cli.py                      the `repoatlas` command
  core.py                     the language-neutral pipeline: scan, resolve, classify, group, render
  viewer.html                 the interactive page (plain CSS and JS, no build step)
  languages/                  ONE FILE PER LANGUAGE
    base.py                     Language base class and shared helpers
    python.py  javascript.py  typescript.py  vue.py  svelte.py
    go.py  java.py  kotlin.py  rust.py  c.py  cpp.py
    ruby.py  php.py  csharp.py  swift.py  scala.py  html.py  css.py
tests/                      a fixture repository per language, the command, and the installer
install.sh                  the one-line installer
scripts/                    build_binary.py (single-file executable), personalize.sh, update_formula.sh
packaging/homebrew/         the Homebrew formula
docs/                       screenshots and a finished example
```

`core.py` knows nothing about any particular language. Everything language-specific (what an import looks
like, how it maps to a file, what counts as a function, the colour in the language bar) lives in that
language's file.

## Adding a language

Create `repoatlas/languages/<name>.py`. It is discovered automatically; nothing else needs editing.

```python
import re
from .base import Language, line_of

class Lua(Language):
    name = "Lua"
    extensions = (".lua",)
    color = "#000080"                    # used by the language bar

    def parse(self, f):                  # fill in what the file contains
        f["imports"] = re.findall(r'require\s*\(?\s*["\']([\w.]+)["\']', f["text"])
        for m in re.finditer(r"^(?:local\s+)?function\s+([\w.:]+)", f["text"], re.M):
            f["symbols"].append(dict(k="function", n=m.group(1), sig="", doc="",
                                     line=line_of(f["text"], m.start())))

    def resolve(self, project, f):       # imports -> (files in the repo, outside libraries)
        uses = {p for i in f["imports"] for p in [i.replace(".", "/") + ".lua"] if p in project.paths}
        return uses, set()
```

Optional hooks: `prepare(project)` to read a manifest once (see `go.py`), `manifests`, `entry_by_name`,
`default_layer`, `stem_layers`, `skip_suffixes`. Start from `ruby.py` (smallest) or `go.py` (typical), then add a
fixture to `tests/test_languages.py`.

## Ground rules

* Standard library only, and keep Python 3.8 working (CI runs 3.8, 3.10, 3.12 and 3.14).
* Output must stay deterministic: sort anything that comes out of a `set`.
* Shell scripts must stay POSIX `sh` and pass `shellcheck`.
