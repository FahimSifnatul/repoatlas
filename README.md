# repoatlas

**See how any codebase fits together, one layer at a time.**

Point `repoatlas` at a repository and it writes one self-contained HTML page: an atlas of the project. Start with the big picture, click any layer or part to zoom in, down to a single function. Then follow each program the project runs, step by step, from where it starts to where it ends. No server, nothing leaves your machine.

![The About page for full-stack-fastapi-template](examples/overview.png)

## Install

### macOS, Linux, WSL

No Rust needed:

```sh
curl -fsSL https://github.com/FahimSifnatul/repoatlas/releases/latest/download/install.sh | sh
```

This downloads the latest release binary for your machine, checks its SHA-256, and copies it to `~/.local/bin`, adding that folder to your `PATH` if needed. If there is no binary for your machine it builds from source instead (needs [Rust](https://rustup.rs)). Open a **new terminal** and check:

```sh
repoatlas --version
```

### Windows

1. Download `repoatlas-windows-x86_64.exe` from the [latest release](https://github.com/FahimSifnatul/repoatlas/releases/latest).
2. Rename it to `repoatlas.exe`.
3. Move it to a folder on your `PATH`, then run `repoatlas --version` in a new terminal.

### With cargo (any OS)

Needs [Rust](https://rustup.rs) 1.82 or newer:

```sh
cargo install --locked repoatlas
```

To build the unreleased `master` instead: `cargo install --locked --git https://github.com/FahimSifnatul/repoatlas`. Remove it with `cargo uninstall repoatlas`.

### From a clone

```sh
git clone https://github.com/FahimSifnatul/repoatlas.git
cd repoatlas
git checkout "$(git tag --sort=-v:refname | head -1)"   # the latest release tag; skip for unreleased master
./install.sh
```

The binary is self-contained, so the clone can be deleted afterwards.

### Manual download

Every [release](https://github.com/FahimSifnatul/repoatlas/releases/latest) file works on its own: download it, `chmod +x` it, and put it on your `PATH`. On macOS, if it was downloaded with a browser, also run `xattr -d com.apple.quarantine repoatlas`.

### Options

Shown for the one-line install; with a clone, run `./install.sh` instead of `curl … | sh`.

| Task | Command |
|---|---|
| Install a specific version | `curl -fsSL <url> \| REPOATLAS_VERSION=v2.1.0 sh` |
| Install elsewhere | `curl -fsSL <url> \| REPOATLAS_INSTALL_DIR=/usr/local/bin sh` |
| Leave shell startup files alone | `curl -fsSL <url> \| sh -s -- --no-modify-path` |
| Update | run the install command again |
| Uninstall | `curl -fsSL <url> \| sh -s -- --uninstall` |

`<url>` is `https://github.com/FahimSifnatul/repoatlas/releases/latest/download/install.sh`.

## Use

```sh
repoatlas                                     # map the current folder
repoatlas ~/code/my-app                       # map another folder
repoatlas https://github.com/pallets/flask    # clone, map, clean up
repoatlas . -o map.html --no-open             # choose the file, skip the browser
```

| Option | Meaning |
|---|---|
| `-o, --out FILE` | output file (default `repoatlas.html`) |
| `--no-open` | do not open the result in a browser |
| `--scip FILE` | use a [SCIP](https://github.com/scip-code/scip) index for exact call matching |

The page is one file: email it, attach it, or commit it. A finished sample is [examples/example.html](examples/example.html).

## What you see

1. **About.** The home page: what the project is, its size and languages, web endpoints, the programs it runs and the files they start from, and the outside tools it is built with.
2. **A layer.** Every part of one layer (interface, entry points, logic, data...), with lines showing who uses whom. Hover a box to follow its connections.

   ![Inside one layer](examples/layer.png)

3. **Parts, files, functions.** Click down to classes, functions and web endpoints, and which files rely on them.
4. **How it runs.** One journey per program the project runs (web server, worker, scheduler...), starting where the project itself says it starts (a Dockerfile, a Procfile, a `package.json` script, `main()`). Each journey lists the functions called in order, nested under their callers, marked with what each step reads (arguments, environment, requests, files, database) and writes (output, responses, files, outside calls). Every web endpoint gets its own journey, with its full mounted path and the middleware that runs first. Pick one and its path lights up on the map; press Play to watch it travel.

   ![How it runs](examples/flow.png)

Every box is clickable. Breadcrumbs, Back and `Esc` go up a level. The screenshots above come from [examples/example.html](examples/example.html).

## Languages

Read with [tree-sitter](https://tree-sitter.github.io), so comments and strings are never mistaken for code.

| Language | Finds | Import links |
|---|---|---|
| Python | classes, functions, routes | packages, relative imports, `src/` layouts, console scripts |
| JavaScript, TypeScript, Vue, Svelte | classes, functions, components, routes | relative paths, aliases, index files, workspaces |
| Go | functions, methods, structs, interfaces | `go.mod` modules, package-shared names |
| Java, Kotlin | classes, objects, methods | package and class names |
| Rust | structs, enums, traits, `impl` methods | `mod`, `use crate::...` |
| C, C++ | structs, classes, functions | local `#include "..."` |
| Ruby, PHP, C#, Swift, Scala | classes, methods, functions | class and function names a file uses |
| Shell | functions | scripts it sources or runs |
| HTML, CSS | interface layer | `<script src>`, `href`, `@import` |

A call is matched to a function by language-neutral rules, most certain first: same class, a class named in the call, a written-out path, a named import, a variable of known type, the same file, imported files, the same package, then the only function with that name. Calls into outside libraries are left out. Only when every rule fails does it fall back to the receiver's name, and that step is drawn dashed and labelled as a guess.

repoatlas has no built-in knowledge of any framework. How a project starts comes from its Dockerfile, compose and Kubernetes files, CI YAML, Procfile, Makefile, supervisor/systemd files, and `package.json` / `composer.json` / `pyproject.toml` scripts. What links to what comes from imports, calls, and paths or class names written in strings. Frameworks are recognised from the libraries imported.

Missing a language? See [CONTRIBUTING.md](CONTRIBUTING.md).

## Sharper matching with SCIP (optional)

For compiler-grade call matching, generate an index with the language's SCIP indexer and pass it in (or leave `index.scip` in the repository):

```sh
rust-analyzer scip .          # Rust
scip-python index .           # Python
scip-typescript index         # TypeScript / JavaScript
scip-java index               # Java, Kotlin, Scala
scip-go                       # Go
repoatlas . --scip index.scip
```

Where the index names a definition, it wins. Everything it does not cover keeps the normal rules. Index the whole project: a half-covered repo can lose guessed edges it should keep. The file is only read, never uploaded.

## Good to know

- **It reads code, it does not run it.** Dynamic imports, dependency injection and calls through outside libraries are invisible. Treat the map as a strong guide and confirm anything important in the code.
- **Fast.** Django (2,800 files, 460,000 lines) maps in 5 to 7 seconds on a laptop.
- **Deterministic.** The same repository always produces the same page.
- **Private.** Nothing is uploaded. The page asks Google Fonts for typefaces when online and falls back to system fonts offline.

## Contributing

Bug reports and new languages are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). Released under the [MIT License](LICENSE).
