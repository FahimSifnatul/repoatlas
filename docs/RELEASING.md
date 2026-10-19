# Releasing and distributing repoatlas

## 1. Publish the repository (once)

From the project folder:

```sh
scripts/personalize.sh YOUR_GITHUB_USER      # puts your GitHub username into the README, installer, formula...
git init -b main && git add . && git commit -m "repoatlas 1.0.0"
gh repo create repoatlas --public --source=. --push
#   no gh? create an empty public repo called "repoatlas" on github.com, then:
#   git remote add origin git@github.com:YOUR_GITHUB_USER/repoatlas.git && git push -u origin main
git tag v1.0.0 && git push origin v1.0.0
```

Pushing the tag starts the **release** workflow. It checks that the tag matches `repoatlas.__version__`, runs the
tests, builds the single-file executable, and publishes a GitHub Release with `repoatlas` and `repoatlas.sha256`
attached. Watch it under the repository's Actions tab.

Now the one-line install from the README works for everybody. Until the first release exists, the installer
falls back to building from the `main` branch, so it works straight after the first push too.

Check it as a stranger would, on a machine (or fresh user account) that has never seen the repo:

```sh
curl -fsSL https://raw.githubusercontent.com/YOUR_GITHUB_USER/repoatlas/main/install.sh | sh
# open a new terminal
repoatlas --version
```

## 2. Homebrew

A *tap* is a GitHub repository called `homebrew-<something>` that holds formulas. You can publish one yourself,
without anyone's approval, and people install from it with one command.

**Fill in the formula** (the tag must already be pushed, because Homebrew installs from GitHub's tarball of it):

```sh
scripts/update_formula.sh v1.0.0         # writes the tarball URL and its sha256 into the formula
```

**Create the tap, test the formula locally:**

```sh
brew tap-new YOUR_GITHUB_USER/tap
cp packaging/homebrew/Formula/repoatlas.rb "$(brew --repository YOUR_GITHUB_USER/tap)/Formula/"
brew install --build-from-source YOUR_GITHUB_USER/tap/repoatlas
brew test YOUR_GITHUB_USER/tap/repoatlas
brew audit --strict YOUR_GITHUB_USER/tap/repoatlas
```

**Publish the tap** (the GitHub repository must be named `homebrew-tap`):

```sh
cd "$(brew --repository YOUR_GITHUB_USER/tap)"
git remote add origin git@github.com:YOUR_GITHUB_USER/homebrew-tap.git
git add . && git commit -m "repoatlas 1.0.0" && git push -u origin main
```

Anyone can now run `brew install YOUR_GITHUB_USER/tap/repoatlas`, and `brew upgrade repoatlas` later. The README
already lists this command, so do this step before you advertise the repository.

**How the formula works.** repoatlas only needs Python's standard library, so the formula copies the package into
Homebrew's `libexec` and writes a small launcher in `bin/` that runs it with Homebrew's own Python
(`python@3.13`). There are no dependencies to keep up to date. When `python@3.13` is retired, change its three
mentions in the formula.

A person who installs with both Homebrew and `install.sh` ends up with two copies. The installer warns when
another `repoatlas` comes first on the `PATH`, so they know which one runs.

**Getting into the main Homebrew repository later (optional).** It accepts projects that are stable, tagged,
licensed and reasonably well known (roughly 75 or more stars). Reviewers prefer Python tools to use
`virtualenv_install_with_resources`, which works with this `pyproject.toml`; with no dependencies there are no
`resource` blocks to add. Start with `brew create <tarball url>` and `brew audit --new --strict`.

## 3. Every later release

1. Change `__version__` in `repoatlas/__init__.py` and commit.
2. `git tag v1.1.0 && git push origin main v1.1.0` (the workflow publishes the new executable).
3. `scripts/update_formula.sh v1.1.0`, copy the formula into the tap repository, commit and push.

Everyone who installed with `install.sh` updates by running the same command again.

## 4. PyPI (optional)

The name `repoatlas` is already used on PyPI by a different project, so this package is published as
**repoatlas-tool** (the command and the module are still `repoatlas`):

```sh
python -m pip install build twine
python -m build && twine upload dist/*
pipx install repoatlas-tool
```

Note that both projects would install a command and a module called `repoatlas`, so they cannot live in the
same environment. That is a good reason to think about the name before publishing.
