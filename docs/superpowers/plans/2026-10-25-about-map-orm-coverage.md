# About page, system map, and framework/ORM coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace the Layers tab with an About page, add a full system map to "How it runs", and cover the popular frameworks and ORMs of every supported language.

**Architecture:** Viewer is one `src/viewer.html` fed by `atlas.rs` JSON. About and map are viewer changes plus one new `edges` field per process. Coverage is data in `src/rules.rs` and `EXT_CATS` in `src/tables.rs`, read by existing mechanisms in `parse.rs`, `mounts.rs`, `orm.rs`.

**Tech Stack:** Rust (cargo, tree-sitter, regex, serde_json), single-file HTML/JS viewer.

**Spec:** `docs/superpowers/specs/2026-10-25-about-map-orm-coverage-design.md`

## Global Constraints
- `export PATH="$HOME/.cargo/bin:$PATH"` before any cargo command; write long output to a scratch file and read it.
- `cargo test && cargo clippy -- -D warnings` green before every commit.
- No framework name in logic: only `src/rules.rs` and the `EXT_CATS` block in `src/tables.rs` (test `no_framework_is_hard_coded`).
- Inexact rule-derived links are `guess`, never presented as exact.
- Layers stay in the JSON (`D.layers`); only the UI for them goes.
- Map draws every part: no cap, no "+N more".
- Commit messages: lowercase conventional (`feat:`, `fix:`), ending with the attribution line from the session.

## Review Focus
- A project with no flow (library with no entry points): About must still render and the How it runs tab must not appear.
- A program whose steps all stay in one unit: map shows one box plus inputs/outputs, no crash on zero edges.
- A cyclic call graph between units: map must still lay out (existing back-edge drop in `layout()`).
- A very large program (hundreds of parts): map renders all and scrolls; page stays responsive.
- A part whose parent is a layer node: breadcrumbs and Escape land on About, not a dead layer page.
- A bare `dict.get(...)` or `list.first()` in a repo using a listed ORM: no Database step (negative per ORM).

---

### Task 1: About page replaces Layers

**Files:**
- Modify: `src/viewer.html` (`overview()`, `render()` tabs, `chain()`/crumbs, Escape handler, unused floor CSS)
- Modify: `src/atlas.rs` (fallback summary near `summary = format!(...layers...)`)
- Modify: `src/main.rs` (scan message near line 97)
- Test: `tests/fixtures.rs`

**Interfaces:**
- Consumes: repo JSON `R` (`name, summary, kind, files, loc, parts, routes, langs, runs, entries, stack, libs`), `D.flow.processes`.
- Produces: viewer route `''` renders About; no route renders a layer node. Fallback summary text `A {lang} {kind} with {n} main parts.`

- [ ] **Step 1: Failing test** `fallback_summary_has_no_layers` in `tests/fixtures.rs`: analyze a two-file repo with no README; assert `repo.summary` does not contain `layers` and contains `main parts`.
- [ ] **Step 2:** `cargo test fallback_summary_has_no_layers`. Expected: FAIL.
- [ ] **Step 3:** Change the fallback summary in `atlas.rs` and the scan message in `main.rs` to speak of parts only.
- [ ] **Step 4:** `overview(first)`: remove floors, stack, "Around the app", and the floors hint. Add an "At a glance" `dl` (kind, languages with share, files and lines, parts, web endpoints when `R.routes`, programs as buttons to `@flow/<i>` when `D.flow`, starting files as `ref` chips from `R.entries`). Keep "Built with" directly under it, then the "See how it runs" callout.
- [ ] **Step 5:** `render()`: tab label `About`. Make `chain()`/crumbs skip nodes with `kind === 'layer'`; Escape from a node whose parent is a layer goes to `''`. Delete CSS only the floors used (`.floor`, `.stack`, `.support`, `.fbody`, `.fcount`, `.relies`, `.around`); keep `.station` (hero uses it).
- [ ] **Step 6:** `cargo test && cargo clippy -- -D warnings`. Expected: green. Generate the page for full-stack-fastapi-template, open in Chrome: About shows the glance list and stack, no floors; a part page's crumbs read Overview / part; zero console errors.
- [ ] **Step 7:** Commit `feat: about page replaces the layers tab`.

### Task 2: System map in "How it runs"

**Files:**
- Modify: `src/atlas.rs` (process json near `"parts": parts, "io": io_map`)
- Modify: `src/viewer.html` (`flowView()`, `card()`/`graph()` for pseudo nodes, CSS)
- Test: `tests/fixtures.rs`

**Interfaces:**
- Consumes: `P.steps` with depth and file, the file-to-unit map already used to build `parts`, `io_files`.
- Produces: process field `"edges": [[unit_id, unit_id, count]]` (units differ, `count` >= 1, only units present in `parts`). Viewer pseudo node ids `io:<key>` for `F.io` keys.

- [ ] **Step 1: Failing tests** in `tests/fixtures.rs`: `process_edges_join_units` (a main in `app/` calling a function in `core/`: edges contain `[app, core, 1]`) and `process_edges_empty_for_one_unit` (all steps in one unit: `edges` is `[]`).
- [ ] **Step 2:** Run them. Expected: FAIL (no `edges`).
- [ ] **Step 3:** Compute edges in `atlas.rs` from each step and its nearest shallower step; add to the process json.
- [ ] **Step 4:** In `flowView()` add `sec('The map', ...)` before "Step by step". Ids are `F.parts` plus `io:<key>` pseudo nodes; input pseudo nodes link to the parts in `F.io[key]` and output ones receive links from them, by `IO[key][1]`. Draw with `layout()` + `graph()`; pseudo cards are non-clickable and use the `WAY` colour. Wide maps scroll horizontally. No cap on parts.
- [ ] **Step 5:** `cargo test && cargo clippy -- -D warnings`. Then in Chrome on full-stack-fastapi-template: backend map shows Web requests/Database on the sides, all parts present, box click opens the part, no console errors. Also open a one-unit program and a library repo.
- [ ] **Step 6:** Commit `feat: system map on how it runs`.

### Task 3: Go coverage

**Files:**
- Modify: `src/rules.rs` (route data, `ORMS`, add `read_prefixes`/`write_prefixes: &'static [&'static str]` to `Orm`)
- Modify: `src/orm.rs` (`verbs` closure in `accesses`: a method also counts as read/write when it starts with a listed prefix)
- Modify: `src/tables.rs` (`EXT_CATS` rows)
- Test: `tests/fixtures.rs`

**Interfaces:**
- Produces: `Orm.read_prefixes`, `Orm.write_prefixes` (empty for every existing entry; later tasks consume them).

- [ ] **Step 1: Failing tests**, one per entry, each asserting the exact label or route: Beego, Iris, Hertz route (verb, full path, handler); sqlc (`q.GetUser(ctx, id)` -> `reads database`, `q.CreateUser` -> `writes database`), pgx (`Query`/`QueryRow` read, `Exec` write), xorm, Beego ORM. Negative: `m.Get("k")` on a plain map-like type gives no Database step.
- [ ] **Step 2:** Run. Expected: FAIL for those needing data or prefixes (note any that already pass: they only need the pinned test).
- [ ] **Step 3:** Add `Orm` fields and prefix check; add the entries and `EXT_CATS` rows.
- [ ] **Step 4:** `cargo test && cargo clippy -- -D warnings`. Expected: green.
- [ ] **Step 5:** Commit `feat(rules): more go frameworks and orms`.

### Tasks 4-11: remaining languages

Each task has the same shape as Task 3 (failing test per entry with exact label or route and one negative, data in `rules.rs`, `EXT_CATS` row, full suite green, one commit `feat(rules): more <language> frameworks and orms`). Entries come from the spec's Design section 3. Mechanism changes are allowed only when a failing test shows the existing one cannot express the entry; each such change is described in the commit body.

| Task | Language | Entries to pin | Mechanism notes |
|---|---|---|---|
| 4 | Python | Falcon, Pyramid (`add_route`, `@view_config`), Tornado (handler tuples), Bottle, Quart, Django Ninja; MongoEngine, Beanie, ODMantic, Piccolo, ormar, PyMongo/Motor, asyncpg/psycopg | Tornado tuples may need a handler form |
| 5 | Rust | Warp (`guess`), Salvo, Tide, Loco; rusqlite, tokio-postgres, Rbatis, mongodb | Warp `path!` chains are `guess` only |
| 6 | JS/TS | Express, Koa, Fastify, Hono, NestJS (pin existing); Next.js file routes (`pages/api/**`, `app/**/route.ts`); Prisma (`prisma.user.findMany` -> model from second segment), TypeORM, Sequelize, Mongoose, Drizzle, Knex, MikroORM | Prisma needs the model taken from the member name; Next.js needs a path-derived route (new data row like `ROUTE_FILES`) |
| 7 | Java/Kotlin | Spring MVC (pin), JAX-RS, Micronaut, Ktor `routing {}`; JPA/Hibernate, Spring Data (prefix rule), MyBatis, jOOQ, Exposed, JDBI | Ktor nested `route("/x") { get {} }` uses the closure-group mechanism |
| 8 | C# | ASP.NET controllers (pin) and minimal APIs (`MapGet/MapPost/...`); EF Core, Dapper | `Map*` names added to `ROUTE_CALLS` |
| 9 | Ruby | Rails `config/routes.rb` (`resources`, `get 'x', to:`), Sinatra, Grape; ActiveRecord, Sequel, Mongoid | `routes.rb` added to `ROUTE_FILES`; `resources :users` added to `RESOURCES` |
| 10 | Swift | Vapor routes; Fluent | none expected |
| 11 | Scala | Play `conf/routes`; Slick, Doobie, Quill | Play `conf/routes` is a non-code routes file; if no existing mechanism fits, defer it and say so in the commit |

### Task 12: Real-repo check and wrap-up

- [ ] **Step 1:** Clone into the scratchpad: full-stack-fastapi-template plus one public sample app each for Go, Rust, JS/TS, Java, C#, Ruby (skip a language if no small sample is found, and say so).
- [ ] **Step 2:** For each, run the CLI from `master` and from this branch; compare route count, routes with a resolved handler, Database step count.
- [ ] **Step 3:** Hand-check three journeys per repo; fix rules for anything wrong, with a fixture test per fix.
- [ ] **Step 4:** Update `README.md` (tabs, supported frameworks and ORMs) and replace `docs/layer.png`/`docs/flow.png`/`docs/overview.png` with fresh screenshots of About and the map.
- [ ] **Step 5:** Report the numbers honestly, including repos where nothing improved. Delete the clones. Commit `docs: about page and system map`.
