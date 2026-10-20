# Contributing

## Setup

Install [Rust](https://rustup.rs) (one command, no sudo). Everything else is fetched by `cargo`.

```sh
git clone https://github.com/FahimSifnatul/repoatlas.git && cd repoatlas
cargo run --release -- .                      # run it on itself
cargo test                                    # the fixture repositories and the command
cargo clippy --all-targets -- -D warnings     # CI fails on any warning
python3 -m unittest tests/test_install.py     # the installer, if you touched install.sh
shellcheck -s sh install.sh scripts/*.sh      # lint the shell scripts, if you touched them
```

## Layout

```
src/
  main.rs                   the `repoatlas` command
  atlas.rs                  the pipeline: scan, parse, link, layers, parts, programs, journeys -> one JSON model
  scan.rs                   walk the repository
  lang.rs                   ONE ENTRY PER LANGUAGE: extensions, grammar, entry-point rule, how names are found
  queries/*.scm             ONE QUERY PER GRAMMAR: definitions, calls, comments and strings in the syntax tree
  parse.rs                  syntax tree -> definitions, calls, routes, imports, symbols
  resolve.rs                imports -> files of the repo and outside libraries (per language)
  graph.rs                  calls -> the functions they run, links between files, inputs / outputs per function
  launch.rs                 how the project is started: Dockerfile, compose, Procfile, package.json, Makefile ...
  layers.rs                 layers, parts, files a framework loads by convention
  journey.rs                one program's steps, from where it starts to where it ends
  finder.rs                 repo files by path, module name, command line or run-time pattern
  tables.rs                 data only: layers, file-name words, library catalogue, input / output patterns
  viewer.html               the interactive page (plain CSS and JS, no build step)
tests/fixtures.rs           a small repository per case, and the command
tests/test_install.py       the installer
examples/                   dump.rs (print a syntax tree), calls.rs (print what each call resolves to)
install.sh                  the one-line installer
scripts/                    personalize.sh, update_formula.sh
packaging/homebrew/         the Homebrew formula
docs/                       screenshots and a finished example
```

Only `lang.rs`, `queries/` and `resolve.rs` know about particular languages. The rest works the same for every
language: definitions and calls come from the query, calls are matched to functions by the same rules everywhere,
and launch files, conventions and inputs / outputs are language-neutral. Nothing may name a framework: framework
knowledge lives only in data tables (`EXT_CATS` in `tables.rs`), and a test checks that.

## Adding a language

1. Add the grammar crate: `cargo add tree-sitter-lua`.
2. Write `src/queries/lua.scm`. Start from the grammar's own `queries/tags.scm` if it has one. The captures are:

   | capture | on | means |
   |---|---|---|
   | `@func` / `@class` / `@type` + `@name` | a definition node, its name | a function / class / interface or type |
   | `@impl` + `@name` | a block of methods for a type | Rust `impl`: methods inside belong to `@name` |
   | `@owner` | inside a `@func` match | the class a method belongs to, when written outside it (Go, C++) |
   | `@call` + `@callee` | a call node, the name it calls | `obj.run(x)`: what is before `@callee` is the receiver |
   | `@ref` | an identifier handed to a call | `Thread(target=run)`, `app.get("/x", handler)` |
   | `@guard` | a top-level statement | `if __name__ == "__main__":`-style blocks |
   | `@comment` / `@doc` / `@string` | comments, docstrings, string literals | text that is not code |

   `cargo run --example dump -- <grammar> <file>` prints a file's syntax tree with its node and field names.
3. Add the grammar to `GRAMMARS` and a `Lang` entry to `LANGS` in `src/lang.rs`: name, extensions, colour, and the
   options that fit (`entry_re` for what a program's start looks like, `scope` for where a name is usable without an
   import, `implicit_this` when a bare call inside a class can be a method of it, `interpreters` for `#!` scripts).
4. If its imports name files, add a resolver to `resolve.rs` (see `resolve_c`, the smallest). Otherwise calls are
   matched by name, which is often enough.
5. Add a fixture to `tests/fixtures.rs`. `cargo run --example calls -- <repo> <file>` shows what each call in a
   file was matched to.

## Ground rules

* Output must stay deterministic: iterate sorted maps, or sort anything that comes out of a hash set.
* No step may guess: a call no rule can place is left out, and a program no launch file or entry rule names is not invented.
* Shell scripts must stay POSIX `sh` and pass `shellcheck`.
