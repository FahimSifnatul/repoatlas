# About page, system map, and framework/ORM coverage

## Intent
The viewer opens on a Layers tab of floors that says little about the project. People want to know first what the project is (stack, size, programs), then how it runs. "How it runs" is a step list only; a picture of how a program's parts connect is missing. Framework and ORM rules cover Python, Go, Rust and PHP; the other supported languages still lack their popular ones.

## Decided with the user
- Remove the Layers tab. Replace it with an **About** page: a brief project description with tech stack and other meta.
- "How it runs" gets a **system map** per program: inputs on the left, the parts it passes through joined by call arrows, outputs on the right. **Every part is drawn; no cap and no "+N more".** Boxes open the part on click.
- Layer pages go. Part, folder and file pages stay.
- Frameworks and ORMs: all popular ones for the supported languages. Go, Python, Rust first, then the others.
- Don't overdo: data first, mechanism changes only where a failing test proves one is needed.

## Goals / non-goals / anti-goals
- Goal: About page and map as above; each popular framework's sample shows the correct route verb, full path and handler; each popular ORM's calls show as `reads X` / `writes X` Database steps.
- Non-goals: new languages or grammars, C# and Ruby import resolvers, removing layers from the data model.
- Anti-goals: framework names in logic (`tests/fixtures.rs::no_framework_is_hard_coded` stays green); invented edges (inexact links are `guess`); catalogue padding (an entry needs a test).

## Constraints
- Layers stay in the JSON (`D.layers`): part colours, unit grouping and the `unreached` filter use them.
- All framework and ORM knowledge lives in `src/rules.rs` and `EXT_CATS` in `src/tables.rs`.
- Cargo is at `~/.cargo/bin`; put it on PATH first.

## Design
1. **About** (`src/viewer.html` `overview()`): README summary, an "At a glance" list (kind, languages with share, files and lines, parts, web endpoints, programs linking to their map, starting files), "Built with" (existing `R.stack`, `R.libs`), and the "See how it runs" callout. Tab labels: `About`, `How it runs`. Breadcrumbs, Escape and routing skip layer nodes. `src/atlas.rs` fallback summary and `src/main.rs` message stop saying "layers".
2. **Map**: each process in `flow.processes` gets `"edges": [[from_unit, to_unit, count]]` from its steps (a step's unit to the unit of each step nested under it, when units differ). `flowView()` draws it with the existing `layout()` and `graph()`. Inputs and outputs are non-clickable pseudo nodes from `F.io` and `IO`. Wide maps scroll sideways.
3. **Coverage**, one language group per task, in this order. Each entry: rule data, an `EXT_CATS` row, a fixture test with a negative.
   - Go: Beego, Iris, Hertz; sqlc `Queries`, pgx, xorm, Beego ORM.
   - Python: Falcon, Pyramid, Tornado, Bottle, Quart, Django Ninja; MongoEngine, Beanie, ODMantic, Piccolo, ormar, PyMongo/Motor, asyncpg/psycopg.
   - Rust: Warp (`guess`), Salvo, Tide, Loco; rusqlite, tokio-postgres, Rbatis, mongodb.
   - JS/TS: Express, Koa, Fastify, Hono, NestJS (fixtures for existing support), Next.js file routes; Prisma, TypeORM, Sequelize, Mongoose, Drizzle, Knex, MikroORM.
   - Java/Kotlin: Spring MVC (fixture), JAX-RS, Micronaut, Ktor; JPA/Hibernate, Spring Data, MyBatis, jOOQ, Exposed, JDBI.
   - C#: ASP.NET controllers (fixture) and minimal APIs; EF Core, Dapper.
   - Ruby: Rails `config/routes.rb`, Sinatra, Grape; ActiveRecord, Sequel, Mongoid.
   - Swift/Scala: Vapor, Play `conf/routes`; Fluent, Slick, Doobie, Quill.
4. One mechanism addition expected: `Orm` gets `read_prefixes` / `write_prefixes` (empty elsewhere) for generated method names (sqlc `GetUser`, Spring Data `findByEmail`).

## Left to the builder / my calls
- Exact markup and CSS of About and map; the pseudo-node look.
- Whether ORM names already in `EXT_CATS` make a new `R.orms` field unnecessary (preferred: no new field).
- Edge counting details for the map.
- Which listed frameworks need no new rule because an existing mechanism already covers them (a fixture then pins it).

## Verification
`cargo test && cargo clippy -- -D warnings` after every task. Viewer checked in Chrome on full-stack-fastapi-template (About has no layers; map shows inputs, parts, outputs, all clickable; no console errors). Per language group: before/after route and Database step counts on one public sample repo, reported honestly.
