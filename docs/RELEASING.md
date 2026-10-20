# Releasing and distributing repoatlas

## 1. Publish the repository (once)

From the project folder:

```sh
scripts/personalize.sh FahimSifnatul      # puts your GitHub username into the README, installer, formula...
git init -b master && git add . && git commit -m "repoatlas 2.0.0"
gh repo create repoatlas --public --source=. --push
#   no gh? create an empty public repo called "repoatlas" on github.com, then:
#   git remote add origin git@github.com:FahimSifnatul/repoatlas.git && git push -u origin master
```

Pushing to `master` starts the **release** workflow. When the tag `v` + the `version` in `Cargo.toml` does not exist
yet, it builds and tests the program on each platform (macOS arm64 and x86_64, Linux x86_64 and arm64, Windows
x86_64), creates the tag on that commit, and publishes a GitHub Release with one file per platform attached, each with
its `.sha256` (`repoatlas-macos-aarch64`, `repoatlas-linux-x86_64`, ...). Watch it under the repository's Actions tab.
Pushing a tag such as `v2.0.0` yourself works too; the workflow then checks that it matches `Cargo.toml`.
The Linux files are fully static, so they run on any distribution.

Now the one-line install from the README works for everybody. Until the first release exists, the installer
falls back to building from the `master` branch, so it works straight after the first push too.

Check it as a stranger would, on a machine (or fresh user account) that has never seen the repo:

```sh
curl -fsSL https://raw.githubusercontent.com/FahimSifnatul/repoatlas/master/install.sh | sh
# open a new terminal
repoatlas --version
```

## 2. Homebrew

A *tap* is a GitHub repository called `homebrew-<something>` that holds formulas. You can publish one yourself,
without anyone's approval, and people install from it with one command.

**Fill in the formula** (the tag must already be pushed, because Homebrew installs from GitHub's tarball of it):

```sh
scripts/update_formula.sh v2.0.0         # writes the tarball URL and its sha256 into the formula
```

**Create the tap, test the formula locally:**

```sh
brew tap-new FahimSifnatul/tap
cp packaging/homebrew/Formula/repoatlas.rb "$(brew --repository FahimSifnatul/tap)/Formula/"
brew install --build-from-source FahimSifnatul/tap/repoatlas
brew test FahimSifnatul/tap/repoatlas
brew audit --strict FahimSifnatul/tap/repoatlas
```

**Publish the tap** (the GitHub repository must be named `homebrew-tap`):

```sh
cd "$(brew --repository FahimSifnatul/tap)"
git remote add origin git@github.com:FahimSifnatul/homebrew-tap.git
git add . && git commit -m "repoatlas 2.0.0" && git push -u origin main
```

Anyone can now run `brew install FahimSifnatul/tap/repoatlas`, and `brew upgrade repoatlas` later. The README
already lists this command, so do this step before you advertise the repository.

**How the formula works.** It builds repoatlas from the release tarball with Homebrew's `rust` (needed only while
building) and `cargo install`. The result is one program with nothing to keep up to date.

A person who installs with both Homebrew and `install.sh` ends up with two copies. The installer warns when
another `repoatlas` comes first on the `PATH`, so they know which one runs.

**Getting into the main Homebrew repository later (optional).** It accepts projects that are stable, tagged,
licensed and reasonably well known (roughly 75 or more stars). The formula already follows the usual pattern for
Rust tools (`depends_on "rust" => :build` and `std_cargo_args`). Start with `brew create <tarball url>` and
`brew audit --new --strict`.

## 3. Every later release

1. Change `version` in `Cargo.toml`, run `cargo build` (it updates `Cargo.lock`), commit both, and
   `git push origin master` (the workflow tags `v2.1.0` and publishes the new programs). Pushes that do not change
   the version release nothing.
2. `scripts/update_formula.sh v2.1.0`, copy the formula into the tap repository, commit and push.

Everyone who installed with `install.sh` updates by running the same command again.

## 4. crates.io (optional)

```sh
cargo publish            # then anyone with Rust can run: cargo install repoatlas
```

Check the name is free on [crates.io](https://crates.io/crates/repoatlas) first.
