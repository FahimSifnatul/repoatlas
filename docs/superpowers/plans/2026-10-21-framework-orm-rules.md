# Framework and ORM Rules (Tier 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Journeys for Python, Go, Rust and PHP projects show real web routes (full path, real handler) and ORM calls as `reads X` / `writes X` Database steps.

**Architecture:** All framework and ORM knowledge moves into one data file, `src/rules.rs`. The parser and graph read it through general mechanisms (handler arguments, attribute markers, routes files, closure groups, ORM model/handle/verb matching). Nothing in the logic names a framework.

**Tech Stack:** Rust, tree-sitter queries (`src/queries/*.scm`), `regex`, existing `serde_json` atlas output. Toolchain: `export PATH="$HOME/.cargo/bin:$PATH"`.

**Spec:** `docs/superpowers/specs/2026-10-21-framework-orm-rules-design.md`

**Scope:** This plan is tier 1 (Python, Go, Rust, PHP). Tier 2 (JS/TS, Java/Kotlin, C#, Ruby) and tier 3 (Swift, Scala) get their own plans after this one ships, because they reuse the mechanisms built here and only add data plus the C# and Ruby resolvers.

## Global Constraints
- No framework or ORM name in logic: `tests/fixtures.rs::no_framework_is_hard_coded` must pass. Names live in `src/rules.rs` and the `EXT_CATS` catalogue only.
- Every rule-derived link that is not exact sets the `guess` flag (dashed in the viewer). Never a solid invented edge.
- No new grammar crates, no new dependencies.
- All 59 existing tests and `cargo clippy -- -D warnings` stay green after every task.
- DB step labels are exactly `reads <Model>` / `writes <Model>`, or `reads database` / `writes database` when the model is unknown.

## Review Focus
- `dict.get`, `list.filter`, `set.add`, `queue.put` must never become Database steps (task 6 negative test).
- A route string without a leading `/` outside a routes file (`logger.get("name")`) is not a route (task 3).
- Two routers with the same variable name in different files get different prefixes (task 4).
- A model class that is only imported, never queried, adds no step to the importing function (task 6).
- Handler named but not found in the repo gives a route journey with no handler steps, not a crash or a wrong edge (task 2).
- A repo with no framework at all produces the same atlas as before (task 1 snapshot).

---

### Task 1: Move route and mount tables into `src/rules.rs`

**Files:**
- Create: `src/rules.rs`
- Modify: `src/tables.rs:135-157`, `src/mounts.rs:16-18`, `src/lib.rs`, `tests/fixtures.rs:399-417`
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces: `rules::ROUTE_RES`, `rules::DECORATOR_ROUTES`, `rules::ROUTE_CALLS`, `rules::ROUTE_HINT`, `rules::MOUNT_CALLS`, `rules::GROUP_CALLS`, with the same types and values as today.

- [ ] **Step 1: Snapshot test.** Add `rule_move_changes_nothing` to `tests/fixtures.rs`: analyze a small FastAPI + Express + Gin fixture and assert the full `routes` JSON of each program equals the literal currently produced (run once, paste the literal).
- [ ] **Step 2:** Run `cargo test rule_move_changes_nothing`. Expected: PASS (it pins today's behaviour).
- [ ] **Step 3:** Create `src/rules.rs` with the moved items; re-export from `tables.rs` is not allowed, so update the `use` lines in `parse.rs`, `layers.rs`, `mounts.rs`.
- [ ] **Step 4:** In `no_framework_is_hard_coded`, strip `src/rules.rs` the same way the `EXT_CATS` block is stripped (skip that file by name).
- [ ] **Step 5:** `cargo test && cargo clippy -- -D warnings`. Expected: all green.
- [ ] **Step 6:** Commit `refactor: move route and mount tables to rules.rs`.

### Task 2: Route handlers, and Go 1.22 verb paths

**Files:**
- Modify: `src/parse.rs:70-79` (Route), `src/parse.rs:541-548`, `src/graph.rs` (resolve handler), `src/rules.rs`
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces: `Route.handler: String` (the raw handler expression, "" when none); `rules::HANDLER_FORMS: &[(&str /*regex*/, HandlerKind)]` with `enum HandlerKind { Name, Dotted, ClassMethod, AtString, Wrapped }`; `fn handler_of(args: &[String]) -> Option<(Option<String> /*class*/, String /*function*/)>` in `parse.rs`.
- Consumes: `mounts::args_of` (make it `pub(crate)`).

- [ ] **Step 1: Failing tests** in `tests/fixtures.rs`:
  - `go_122_patterns`: `mux.HandleFunc("GET /users/{id}", getUser)` gives route verb `GET`, path `/users/{id}`, handler `getUser` as the first step of the route journey.
  - `handler_forms`: for each of `get(h)` wrapped (Axum `.route("/x", get(h))`), `[UserController::class, 'index']`, `'UserController@index'`, `views.profile`, the route journey's first step is that function.
  - `unknown_handler_is_harmless`: handler not in the repo gives a route with zero steps and no panic.
- [ ] **Step 2:** Run them. Expected: FAIL (no handler on routes; Go pattern dropped because the string does not start with `/`).
- [ ] **Step 3:** Relax `FIRST` (`parse.rs:542`) so the path may be preceded by `VERB ` inside the string; set the route verb from that prefix when present.
- [ ] **Step 4:** Fill `Route.handler` from the call's later arguments using `HANDLER_FORMS`; in `graph.rs`, resolve it with `Index::qualified` and mark the resulting call `guess` unless the class/function lookup is unique.
- [ ] **Step 5:** Run the three tests. Expected: PASS. Run full suite.
- [ ] **Step 6:** Commit `feat: route handlers and go 1.22 patterns`.

### Task 3: Attribute routes with data-driven markers; routes files

**Files:**
- Modify: `src/rules.rs`, `src/parse.rs:316-348`, `src/parse.rs:300-314` (`head_of`)
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces: `rules::ATTR_MARKERS: &[&str]` (`"@"`, `"#["`, `"["`); `rules::ROUTE_FILES: &[(&str /*path glob*/, &str /*kind*/)]` (`urls.py`, `routes/*.php`, `config/routes.rb`); `fn is_routes_file(path: &str) -> bool` in `parse.rs`.

- [ ] **Step 1: Failing tests:**
  - `rust_attribute_routes`: `#[get("/users")] async fn list()` and `#[route("/x", method = "GET")]` yield routes `GET /users`, `GET /x`.
  - `php_attribute_routes`: `#[Route('/api')] class A { #[Route('/users', methods: ['GET'])] function list() }` yields `GET /api/users`.
  - `routes_file_without_leading_slash`: in `app/urls.py`, `path("users/", views.users)` is route `ANY /users/`; the same call in `app/util.py` is not a route (Review Focus).
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Generalise `DECORATOR_ROUTES` matching so the marker prefix comes from `ATTR_MARKERS`; extend `class_prefix` to read the `Route` attribute from data. In `is_routes_file` files, apply call-route detection without the leading-`/` check.
- [ ] **Step 4:** Run tests and full suite. Expected: PASS.
- [ ] **Step 5:** Commit `feat: attribute routes and routes files`.

### Task 4: Includes and closure groups

**Files:**
- Modify: `src/mounts.rs` (`build`, `Edge`), `src/rules.rs`
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces: `rules::INCLUDE_CALLS: &[&str]` (`include`), `rules::CLOSURE_GROUPS: &[(&str /*call regex*/, &str /*prefix arg*/)]`; `Mounts::prefix_at(fi: usize, at: usize, recv: &str) -> String` (route prefix including any closure group that encloses byte `at`).
- Consumes: `Mounts::prefix(fi, recv)`.

- [ ] **Step 1: Failing tests:**
  - `django_include`: `urls.py` has `path("api/", include("shop.urls"))`; `shop/urls.py` has `path("items/", views.items)`; route path is `/api/items/`.
  - `laravel_group`: `Route::prefix('api')->group(function () { Route::get('/users', ...); });` gives `/api/users`.
  - `chi_route_group`: `r.Route("/v1", func(r chi.Router) { r.Get("/x", h) })` gives `/v1/x`.
  - `same_router_name_two_files`: two files each with `router`, mounted at different prefixes, keep their own prefixes.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Resolve the `include("pkg.mod")` string to a file through the existing module lookup used by the Python resolver (`resolve.rs`); add an edge with the call's prefix. For closure groups, record `(byte range, prefix)` per file and add the enclosing prefix in `prefix_at`.
- [ ] **Step 4:** Run tests and full suite. Expected: PASS.
- [ ] **Step 5:** Commit `feat: include and closure-group route prefixes`.

### Task 5: PHP PSR-4 resolver

**Files:**
- Modify: `src/resolve.rs`, `src/lang.rs` (add `Resolver::Php`, set on PHP, add `composer.json` manifest), `src/scan.rs` if manifests are read there
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces: `Resolver::Php`. A `use App\Http\Controllers\UserController;` resolves to the file whose path matches a `composer.json` `autoload.psr-4` root (`"App\\": "app/"` gives `app/Http/Controllers/UserController.php`).

- [ ] **Step 1: Failing test** `php_use_follows_psr4`: fixture with `composer.json` and two PHP files; the controller's imports list the model file.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Read `autoload.psr-4` (and `autoload-dev`) from every `composer.json`; map the longest matching namespace prefix to its directory. Unmatched namespaces stay unresolved.
- [ ] **Step 4:** Run test and full suite. Expected: PASS.
- [ ] **Step 5:** Commit `feat: resolve php namespaces from composer psr-4`.

### Task 6: ORM detection with read/write steps

**Files:**
- Modify: `src/rules.rs`, `src/graph.rs:866-930` (`def_io`), `src/tables.rs` (`IO`), `src/viewer.html`
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Orm {
      pub name: &'static str,            // display, e.g. "Django ORM"
      pub lang: &'static str,            // lang::Lang.name
      pub model_bases: &'static [&'static str],   // base class names / derives / embedded fields
      pub model_attrs: &'static [&'static str],   // attribute / annotation text
      pub handles: &'static [&'static str],       // declared types / variable names
      pub entry: &'static [&'static str],         // accessors: "objects", "query", "where"
      pub reads: &'static [&'static str],
      pub writes: &'static [&'static str],
  }
  pub static ORMS: &[Orm];
  ```
  `fn db_access(f: &File, call: &Call, models: &HashSet<String>) -> Option<(&'static str /*"read"|"write"*/, Option<String> /*model*/)>` in `graph.rs`. `IO` gains `db_read` and `db_write` display keys that roll up into `db` for layers.
- Consumes: `Def.bases`, `File.var_types`, `Call.recv`.

- [ ] **Step 1: Failing tests** (one per case, each asserts the exact step label):
  - `django_orm`: `User.objects.filter(...)` in a view gives `reads User`; `user.save()` where `user` was assigned from `User.objects.get` gives `writes User`.
  - `negative_plain_collections`: `d.get("a")`, `items.filter(f)` on a list, `s.add(1)` give no Database step.
  - `imported_model_unused`: a function that only imports `User` adds no step.
  - `sqlalchemy_session`: `db.execute(select(User))` with `db: Session` gives `reads User`; `db.add(u); db.commit()` gives `writes User` (or `writes database` when the model is unknown).
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Collect model classes per repo from `ORMS[..].model_bases/model_attrs` matched against `Def.bases` and decorator head text. In `def_io`, for each call whose receiver chain starts at a model, an `entry` accessor, or a variable declared as a `handles` type, classify the last method with `reads`/`writes`; push `db_read` or `db_write` and add `db` to the rolled-up set. Step label uses the model name when known.
- [ ] **Step 4:** Update `viewer.html` to show the `reads`/`writes` label on the existing Database colour.
- [ ] **Step 5:** Run tests and full suite. Expected: PASS.
- [ ] **Step 6:** Commit `feat: orm models, handles and read/write database steps`.

### Task 7: Tier 1 framework and ORM data, one language at a time

**Files:**
- Modify: `src/rules.rs`, `src/tables.rs` (`EXT_CATS` additions)
- Test: `tests/fixtures.rs` (one fixture test per row)

**Interfaces:** consumes everything above; adds data only.

Each row below is its own sub-task: write the fixture test first (route verb, full path, handler, DB steps), see it fail, add the data, see it pass, commit `feat(rules): <name>`.

- [ ] **Python frameworks:** Django (+DRF `router.register`), Flask blueprints, Starlette, aiohttp, Sanic, Litestar.
- [ ] **Python ORMs:** Django ORM, SQLAlchemy 1.x and 2.0, SQLModel, Peewee, Tortoise, Pony.
- [ ] **Go frameworks:** net/http, Gin, Echo, Fiber, chi, gorilla/mux.
- [ ] **Go ORMs:** GORM, sqlx, database/sql, ent, bun, sqlc `Queries`.
- [ ] **Rust frameworks:** Axum, Actix-web, Rocket, Poem (Warp as `guess`).
- [ ] **Rust ORMs:** Diesel, SQLx (`query!`, `query_as!`, `fetch_*`, `execute`), SeaORM.
- [ ] **PHP frameworks:** Laravel (routes files, `Route::resource`, groups, `[Ctrl::class, 'm']`), Symfony attributes, Slim, Lumen.
- [ ] **PHP ORMs:** Eloquent, Doctrine (entities, repositories, `EntityManager`), PDO.

### Task 8: Real-repo before/after check

**Files:** none changed; results go in the final report.

- [ ] **Step 1:** Clone into the scratchpad: full-stack-fastapi-template, a Django/DRF realworld app, a gin or chi realworld app, an axum and an actix realworld app, a Laravel realworld app, symfony-demo.
- [ ] **Step 2:** For each, run the binary built from `fix/language-neutral-mapping` and from this branch with `examples/dump.rs` or the CLI; compare route count, routes with a resolved handler, and Database step count.
- [ ] **Step 3:** Hand-check five journeys per repo (path correct, handler correct, DB labels correct). Fix rules for anything wrong, with a fixture test per fix.
- [ ] **Step 4:** Report the numbers honestly, including any repo where nothing improved. Delete the clones.
