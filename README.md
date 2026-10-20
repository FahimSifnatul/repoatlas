# repoatlas

**See how any codebase fits together, one layer at a time.**

Point `repoatlas` at a repository and it writes a single HTML page that works like an atlas of the project. Start with the big picture, click any layer or part to zoom in, and keep going down to a single function. Then follow each program the project runs, step by step, from where it starts to where it ends. No setup, no server, nothing leaves your machine.

![The overview: one floor per layer](docs/overview.png)

## Install

macOS and Linux (and Windows through WSL). No sudo needed:

```sh
curl -fsSL https://raw.githubusercontent.com/FahimSifnatul/repoatlas/master/install.sh | sh
```

Then open a **new terminal window** and check it worked:

```sh
repoatlas --version
```

That is the whole install. It downloads **one program** made for your machine into `~/.local/bin` and adds that folder to your `PATH` if it was not already there. It needs nothing else: no Python, no Node, no runtime. Nothing else on your computer is touched, and you do not need to keep anything you downloaded.

<details>
<summary><b>Other ways to install</b></summary>

**From a clone, then delete the clone.** This builds it with [Rust](https://rustup.rs)'s `cargo`. Because `repoatlas` is a single self-contained program, the folder is not needed afterwards:

```sh
git clone https://github.com/FahimSifnatul/repoatlas.git
cd repoatlas && ./install.sh
cd .. && rm -rf repoatlas
repoatlas --version          # still works
```

**Homebrew**

```sh
brew install FahimSifnatul/tap/repoatlas
```

**Windows without WSL:** download `repoatlas-windows-x86_64.exe` from the [latest release](https://github.com/FahimSifnatul/repoatlas/releases/latest) and rename it to `repoatlas.exe`. Or, with Rust installed: `cargo install --git https://github.com/FahimSifnatul/repoatlas`.

**A specific version, or another folder**

```sh
curl -fsSL https://raw.githubusercontent.com/FahimSifnatul/repoatlas/master/install.sh | REPOATLAS_VERSION=v2.0.0 sh
curl -fsSL https://raw.githubusercontent.com/FahimSifnatul/repoatlas/master/install.sh | REPOATLAS_INSTALL_DIR=/usr/local/bin sh
```

**Update:** run the install command again. **Uninstall:**

```sh
curl -fsSL https://raw.githubusercontent.com/FahimSifnatul/repoatlas/master/install.sh | sh -s -- --uninstall
```

**Want to read the installer first?** It is one short file: [install.sh](install.sh). It also checks the download against a published checksum before installing it. On a machine with no ready-made program (a less common processor), it builds one from source with `cargo`.

</details>

## Use

```sh
repoatlas                       # map the folder you are in
repoatlas ~/code/my-app         # map a project somewhere else
repoatlas https://github.com/pallets/flask    # map a repository straight from GitHub
repoatlas . -o map.html --no-open             # choose the file name, do not open the browser
```

It writes `repoatlas.html` and opens it in your browser. The file is self-contained, so you can email it, attach it to a ticket, or commit it next to the code.

## What you see

**1. The overview.** One floor per layer of the project: what people see, the front door, the brain, the memory, the phone lines to the outside world, the shared toolkit. Upper floors rely on the floors below them.

**2. Zoom into a layer.** Every part of that layer, with lines showing who uses whom. Hover a box to follow its connections.

![Inside one layer](docs/layer.png)

**3. Keep going.** Click a part to see its folders and files, and a file to see its classes, functions and web endpoints, and exactly which files rely on it.

**4. How it runs.** The "How it runs" tab shows one journey per program the project runs, for example a web server, a background worker and a scheduler. Each starts where the project itself says it starts, for example "Dockerfile runs entrypoint.sh, which runs `gunicorn app.wsgi`", at the exact function that runs first. From there it lists every function the program calls, in the order the code calls them, each one indented under the function that calls it, down to where its input comes in and its output goes out. Each step is marked with what it takes in (command-line arguments, environment variables, web requests, files, a database) and what it gives out (terminal output, web responses, files, calls to other services and programs). A call that depends on the object it is made on shows every class it can reach ("parse() — one of 6"). A function already shown links back to its first appearance instead of repeating it. For a server, each web endpoint gets its own journey: what runs when that request arrives. Anything no journey reaches is listed at the end instead of being guessed.

![How it runs](docs/flow.png)

Web endpoints show their **full path**: the prefix of every router they are mounted under is joined in (`app.include_router(items.router, prefix=settings.API_V1_STR)`, `app.use('/api', routes)`, `r.Group("/v1")`, a controller class's `@RequestMapping("/users")`). A prefix kept in a constant is looked up; when the project does not give it one value, it is shown by name, as `{API_V1_STR}`. What runs before the handler is listed first: middleware handed to the route, and functions a signature names through an alias (`SessionDep = Annotated[Session, Depends(get_db)]`, then `def read(session: SessionDep)`).

Every box is clickable. The breadcrumb, your browser's Back button and the `Esc` key all take you up a level.

A finished example is in [docs/example.html](docs/example.html): download it and open it in a browser.

## Languages

Every language below is read with a real parser ([tree-sitter](https://tree-sitter.github.io)), so comments and strings are never mistaken for code, and every call is matched to the function it runs.

| Language | What it finds | Import links |
|---|---|---|
| Python | classes, methods, functions, web routes | packages, relative imports, `src/` layouts, console scripts |
| JavaScript, TypeScript, Vue, Svelte | classes, methods, functions, components, types, routes | relative paths, `@/` aliases, index files, workspace packages |
| Go | functions, methods, structs, interfaces | module imports via `go.mod`, names shared inside a package |
| Java, Kotlin | classes, objects, methods, functions | package and class names, classes shared inside a package |
| Rust | structs, enums, traits, functions, `impl` methods | `mod` and `use crate::...` |
| C, C++ | structs, classes, functions, methods | local `#include "..."` |
| Ruby, PHP, C#, Swift, Scala | classes, methods, functions | the class and function names a file uses |
| Shell | functions | scripts it sources or runs |
| HTML, CSS | counted as the interface layer | `<script src>`, `href`, `@import` |

A call is matched to a function by rules that work the same in every language, most certain first: a method of the same class (or of a class built on it), a class named in the call (`Foo.make()`, `Foo::new()`), a path written out (`crate::graph::link()`, `pkg.mod.run()`, a name re-exported through `lib.rs` or `__init__.py`), a name imported one by one (`from crud import make as build`), a variable or parameter whose type is declared or built (`p: Parser`, `p = Parser()`, `this.svc` set by the constructor), the same file, the files it imports, the same package, and the only function with that name in the project. A call that leads into an outside library (`fs::File::open`, `os.path.join`) is left out. Only when every rule fails does it fall back to the name of the object (`project.run()` can be a `Project`'s method): such a step is drawn with a dashed name and says it was guessed from the name. A step also says when its call only runs in some cases, in a loop, or when something fails, and for a web endpoint what runs before the handler (auth and other middleware handed to the route).

repoatlas has no built-in knowledge of any framework. It works out how a project starts and fits together from rules every framework shares:

* **How it is started.** Commands written in a Dockerfile, docker-compose, Kubernetes or CI YAML, a Procfile, a Makefile, supervisor or systemd files, nginx / Apache web roots, `package.json` / `composer.json` / `pyproject.toml` scripts, or built by the code itself. It also follows the folder each command runs in (Docker `WORKDIR` and `COPY`, compose `working_dir` and volumes, `chdir`), what an image copies in and builds, and arguments that name a file of the program (`serve` → `cmd/serve.go`). Without any of these, `main()` and `#!` scripts are the starting points.
* **What links to what.** Imports and calls, plus file paths, module names and class names written in strings (templates, settings modules, `"html/ex%d.html"`, `'AuthController@login'`).
* **Conventions.** A file name repeated in sibling packages that nothing imports, a folder of look-alike files nothing imports, or the folder below a start that imports nothing. These are files the framework loads on its own. Numbered files such as migrations are left to the separate command that runs them.
* **The framework itself.** Named from the libraries the code imports and the project's dependency files.

Missing yours? A language is a tree-sitter grammar, one small query file and one table entry, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Sharper matching with a SCIP index (optional)

repoatlas works out which function a call runs with its own rules, and shows the calls it had to guess. If you want compiler-grade answers, give it a [SCIP](https://github.com/scip-code/scip) index. Each language has an indexer that writes one `index.scip` file in a single run:

```sh
rust-analyzer scip .          # Rust
scip-python index .           # Python
scip-typescript index         # TypeScript / JavaScript
scip-java index               # Java, Kotlin, Scala
scip-go                       # Go
repoatlas . --scip index.scip # or just leave index.scip in the repository: it is picked up by itself
```

Where the index names the definition of a call, that definition is used (and a call on an interface runs every implementation the index lists, up to six). A call the index places in an outside library replaces a guess made from a name. Everything the index does not cover keeps the normal rules, so a partial index is safe, but a repo it only half covers may lose a guessed edge it should keep, so index the whole project. Nothing is run or uploaded: repoatlas only reads the file.

## Good to know

* **It reads your code, it does not run it.** The layers come from folder and file names, the libraries a file imports, and its imports of other files. The journeys come from the calls each function makes. Inputs and outputs come from what each function's own code does (`open(`, `fetch(`, `os.Getenv` ...), with comments and docstrings ignored. That is a very good guide to how a project works, not a guarantee, so confirm anything important in the code.
* **Dynamic imports, dependency injection and calls through outside libraries are invisible** to a tool that does not run the program. When a framework calls your code (a request handler, a page component), the journey says where that code is registered instead of inventing who calls it.
* **Fast.** Django (2,800 files, 460,000 lines) maps in 5 to 7 seconds on a laptop.
* **Nothing is uploaded.** The page does ask Google Fonts for its typefaces when you are online, and falls back to your system fonts when you are not.
* Output is deterministic: the same repository always produces the same page.

## Contributing

Bug reports and new languages are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Maintainers: [docs/RELEASING.md](docs/RELEASING.md).

Released under the [MIT License](LICENSE).
