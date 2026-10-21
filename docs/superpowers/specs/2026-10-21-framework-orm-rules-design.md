# Framework and ORM rules for repoatlas

## Intent
Journeys should show real web routes (full path, real handler) and real database access (read or write) for the popular frameworks and ORMs of the supported languages. Today only raw SQL and a few route shapes are understood.

## Decided with the user
- Rules change journeys. Recognition-only catalogue entries are not the goal.
- Database steps say whether they read or write.
- No new languages or grammars.
- Priority: Python, Go, Rust, PHP. Then JS/TS, Java/Kotlin, C#, Ruby. Then Swift, Scala.

## Goals / non-goals / anti-goals
- Goal: for each priority framework, a sample app's route journeys show the correct full path and handler, and its ORM calls show as read or write Database steps.
- Non-goals: new grammars, LSP, runtime tracing, catalogue padding.
- Anti-goals: invented edges (rule-derived links are exact or flagged `guess`); framework names in logic; one ever-growing regex per framework instead of a general mechanism.

## Constraint
`tests/fixtures.rs::no_framework_is_hard_coded` forbids framework names in logic. All framework and ORM knowledge lives in data (`src/rules.rs`), read by general mechanisms.

## Design
1. `src/rules.rs`: data-only. Existing route and mount tables move there first, with no behaviour change.
2. Route mechanisms: call routes with handler arguments (name, `views.foo`, `[Ctrl::class, 'm']`, `Ctrl@m`, `get(handler)`, `to: 'a#b'`); attribute routes with a data-driven marker (`@`, `#[`, `[Http..]`) and class prefixes; routes files matched by path (`urls.py`, `routes/*.php`, `config/routes.rb`, `conf/routes`); nested prefixes (includes, closure groups, resource expansion).
3. ORM mechanisms: model markers (base, attribute/derive, embedded field), handle types, read/write verb lists, query-entry accessors. A call on a model, a query chain or a handle becomes a Database step labelled `reads X` / `writes X`. `db_read` and `db_write` roll up into the existing `db` key.
4. Tiers: (1) Python, Go, Rust, PHP; (2) JS/TS, Java/Kotlin, C#, Ruby; (3) Swift, Scala. Framework and ORM lists per language are in the approved plan.
5. Language fixes found by the audit: PHP PSR-4 resolver (tier 1), C# resolver and csproj manifest, Ruby require_relative/Zeitwerk mapping.

## Left to the builder / my calls
- `rules.rs` holds all framework and ORM data; `tables.rs` keeps layers, I/O and the catalogue.
- New `EXT_CATS` entries accompany each framework and ORM.
- Inexact rule-derived links are `guess` (dashed).

## Verification
`cargo test && cargo clippy -- -D warnings` after each step; a fixture test per framework and ORM (route verb, full path, handler, `reads X`/`writes X`, plus negatives like `dict.get`); before/after runs on public sample repos kept in the scratchpad.
