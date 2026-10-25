//! Data only: how web frameworks write routes and mount them on each other.
//! Framework names may appear here and in tables.rs, never in the logic (a test checks that).

use std::sync::LazyLock;

use regex::Regex;

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

/// catalogue names (tables::EXT_CATS) that count only where code imports them: as a dependency or a command they are
/// something else (`ninja` imported is Django Ninja, whose package is `django-ninja`; listed or run, it is the build tool)
pub const IMPORT_ONLY_NAMES: &[&str] = &["ninja"];

/// calls that put one router under another
pub const MOUNT_CALLS: &[&str] = &["include_router", "register_blueprint", "use", "mount", "Mount", "add_router"];

/// the language names of JavaScript and TypeScript files (.js .jsx .mjs .cjs, .ts .tsx)
const JS_TS: &[&str] = &["JavaScript", "TypeScript"];
/// calls that put one router under another in files of these languages only: Fastify `fastify.register(users, { prefix: '/users' })`,
/// Hono `app.route('/users', users)` (a `route` call given a path and a router is a mount, not an endpoint)
pub const LANG_MOUNT_CALLS: &[(&[&str], &[&str])] = &[(JS_TS, &["register", "route"])];
/// languages whose routers take their options as one object: Koa `new Router({ prefix: '/api' })`, Fastify `{ prefix: '/users' }`
pub const OPTION_OBJECT_LANGS: &[&str] = JS_TS;

/// whether a call of that name, in a file of that language, puts one router under another
pub fn mount_call(lang: &str, name: &str) -> bool {
    MOUNT_CALLS.contains(&name) || lang_mount_call(lang, name)
}
/// whether a call of that name mounts a router in that language only (`route` elsewhere registers an endpoint)
pub fn lang_mount_call(lang: &str, name: &str) -> bool {
    LANG_MOUNT_CALLS.iter().any(|(ls, cs)| ls.contains(&lang) && cs.contains(&name))
}
/// calls that make a sub-router with a prefix: `v1 := r.Group("/v1")`
pub const GROUP_CALLS: &[&str] = &["Group", "group", "Party"];

pub static ROUTE_HINT: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"@(?:\w+\.)?(?:route|get|post|put|delete|patch)\(|\b(?:app|router)\.(?:get|post|put|delete|patch|use)\(|@RestController|@Controller|@(?:Get|Post|Put|Delete|Patch|Request)Mapping|HandleFunc\(|urlpatterns")
});
/// app.get("/x"), Route::get("/x"), @GetMapping("/x"), r.GET("/x"), HandleFunc("/x")
pub static ROUTE_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        rx(r#"\b(?:app|router|routes?|Route)(?:\.|->|::)(get|post|put|delete|patch|all|any)\(\s*['"`]([^'"`]+)['"`]"#),
        rx(r#"@(Get|Post|Put|Delete|Patch)Mapping\(\s*(?:value\s*=\s*|path\s*=\s*)?"([^"]*)""#),
        rx(r#"\.(GET|POST|PUT|DELETE|PATCH)\(\s*"([^"]+)""#),
        rx(r#"HandleFunc\(\s*"([^"]+)""#),
    ]
});
/// what opens an attribute: `@` (decorators, annotations) or `#[` with an optional path (`#[actix_web::get(..)]`, `#[Route(..)]`)
const ATTR: &str = r"(?:@|#\[\s*(?:[\w:\\]*(?:::|\\))?)";

/// a route written above a function: @app.get("/x"), @router.post(...), @GetMapping("/x"), [HttpGet("x")], #[get("/x")]
pub static DECORATOR_ROUTES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        rx(&format!(r#"{ATTR}(?:[\w.]+\.)?(?i:(get|post|put|delete|patch|route|head|options|websocket))\(\s*[rbuf]*['"]([^'"]+)['"]"#)),
        rx(r#"@(Get|Post|Put|Delete|Patch|Request)Mapping\(\s*(?:value\s*=\s*|path\s*=\s*)?"([^"]*)""#),
        rx(r#"\[Http(Get|Post|Put|Delete|Patch)\(\s*"([^"]*)""#),
    ]
});
/// call names that register a web endpoint: app.get("/x", handler), mux.HandleFunc("/x", h)
pub const ROUTE_CALLS: &[&str] = &["get", "post", "put", "delete", "patch", "all", "any", "route", "handle", "HandleFunc", "Handle",
    "GET", "POST", "PUT", "DELETE", "PATCH", "Get", "Post", "Put", "Delete", "Patch", "at", "add_route", "add_api_route",
    "add_url_rule", "add_get", "add_post", "add_put", "add_delete", "add_patch", "Router"];
/// call names that register a route only when called bare: `Route("/x", handler)` (a method called `Route` makes a group instead)
pub const ROUTE_BARE: &[&str] = &["Route"];
/// the keyword that carries the handler: `add_url_rule("/x", view_func=users)`
pub const HANDLER_KWARGS: &[&str] = &["view_func", "endpoint", "handler", "controller", "to", "uses"];
/// the verb written after the call: `r.HandleFunc("/x", h).Methods("GET")`
pub static METHODS_AFTER: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*\.\s*Methods\(\s*"(\w+)""#));

/// calls that register a whole controller: `Route::apiResource('photos', PhotoController::class)`,
/// `router.register(r"users", UserViewSet)`. Each action: (verb, what follows the name, method called)
pub const RESOURCES: &[(&str, &[(&str, &str, &str)])] = &[
    ("resource", &[("GET", "", "index"), ("GET", "/create", "create"), ("POST", "", "store"), ("GET", "/{id}", "show"),
                   ("GET", "/{id}/edit", "edit"), ("PUT", "/{id}", "update"), ("DELETE", "/{id}", "destroy")]),
    ("apiResource", &[("GET", "", "index"), ("POST", "", "store"), ("GET", "/{id}", "show"), ("PUT", "/{id}", "update"), ("DELETE", "/{id}", "destroy")]),
    ("register", &[("GET", "/", "list"), ("POST", "/", "create"), ("GET", "/{pk}/", "retrieve"), ("PUT", "/{pk}/", "update"), ("DELETE", "/{pk}/", "destroy")]),
];

/// how a route's handler is written, the last argument of the registering call: the class (if any) and the function.
/// `[UserController::class, 'index']`, `'UserController@index'`, `ctrl.profile`, `list_items`
pub static HANDLER_FORMS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // a class-based view: every verb method of the class runs
        rx(r"^(?:[\w.]+\.)?(?P<class>\w+)\.as_view\(.*\)$"),
        // a controller value: `&UserController{}`
        rx(r"^&?(?:\w+\.)*(?P<class>\w+)\{\s*\}$"),
        rx(r#"^\[\s*(?:[\w\\]+\\)?(?P<class>\w+)(?:::class|\.class)?\s*,\s*['"](?P<func>\w+)['"]\s*\]$"#),
        rx(r#"^['"](?:[\w\\]+\\)?(?P<class>\w+)@(?P<func>\w+)['"]$"#),
        rx(r"^(?:[\w$]+(?:\.|::))*(?P<class>[\w$]+)(?:\.|::)(?P<func>[\w$]+)$"),
        rx(r"^(?P<func>[\w$]+)$"),
    ]
});
/// the methods of a class-based view or viewset that answer requests, by language ("" = any):
/// a Go controller answers with exported methods `Get`, `Post`; a Falcon resource with `on_get`, `on_post`
pub const VERB_METHODS: &[(&str, &[&str])] = &[
    ("", &["get", "post", "put", "patch", "delete", "list", "create", "retrieve", "update", "partial_update", "destroy"]),
    ("Go", &["Get", "Post", "Put", "Patch", "Delete"]),
    ("Python", &["on_get", "on_post", "on_put", "on_patch", "on_delete"]),
];

/// languages where a class (or a new object of it) handed to a route that names no verb answers with its verb methods:
/// Falcon `app.add_route("/x", UserResource())`, Tornado `(r"/x", UserHandler)`
pub const CLASS_HANDLER_LANGS: &[&str] = &["Python"];
/// the verb a class handler's method answers, by language: Falcon `on_get`, Tornado and Starlette `get`
pub const METHOD_VERBS: &[(&str, &[(&str, &str)])] = &[
    ("Python", &[("on_get", "GET"), ("on_post", "POST"), ("on_put", "PUT"), ("on_patch", "PATCH"), ("on_delete", "DELETE"),
                 ("get", "GET"), ("post", "POST"), ("put", "PUT"), ("patch", "PATCH"), ("delete", "DELETE")]),
];

/// the verb a method of that name answers when its class is a route's handler
pub fn method_verb(lang: &str, name: &str) -> Option<&'static str> {
    METHOD_VERBS.iter().filter(|(l, _)| *l == lang).flat_map(|(_, ms)| ms.iter()).find(|(m, _)| *m == name).map(|(_, v)| *v)
}

/// languages where a verb decorator may name no path, and then answers at its class prefix: NestJS `@Post()`
pub const BARE_VERB_DECORATOR_LANGS: &[&str] = JS_TS;
pub static BARE_VERB_DECORATOR: LazyLock<Regex> = LazyLock::new(|| rx(r"@(Get|Post|Put|Delete|Patch)\(\s*\)"));

/// calls whose argument is a list of (path, handler) pairs, by language: Tornado `Application([(r"/x", Handler)])`
pub const ROUTE_TABLES: &[(&str, &str)] = &[("Python", "Application")];
/// one (path, handler) pair of a route table
pub static ROUTE_TABLE_ENTRY: LazyLock<Regex> = LazyLock::new(|| rx(r#"\(\s*[rRbBuU]*['"]([^'"]+)['"]\s*,\s*([\w.]+)\s*[,)]"#));

/// calls that register a route by name, its path second, and a view joined to it elsewhere by that name:
/// Pyramid `config.add_route("users", "/users")` and `@view_config(route_name="users", request_method="GET")`
pub const NAMED_ROUTE_CALLS: &[(&str, &str)] = &[("Python", "add_route")];
pub static NAMED_ROUTE_ARGS: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*\(\s*['"]([\w.:-]+)['"]\s*,\s*['"](/[^'"]*)['"]"#));
/// the decorator of a view that answers a named route, and the verb it answers
pub static VIEW_ROUTE_NAME: LazyLock<Regex> = LazyLock::new(|| rx(r#"@[\w.]*view_config\([^)]*\broute_name\s*=\s*['"]([^'"]+)['"]"#));
pub static VIEW_VERB: LazyLock<Regex> = LazyLock::new(|| rx(r#"@[\w.]*view_config\([^)]*\brequest_method\s*=\s*\(?\s*['"](\w+)['"]"#));

/// A router built by a chain of calls, where one call sets the path the calls chained after it register under:
/// Tide `app.at("/users").get(list)`, Salvo `Router::with_path("users").get(list)`, Loco `Routes::new().prefix("users").add("/", get(list))`
pub struct Chain {
    /// the language (lang::Lang.name)
    pub lang: &'static str,
    /// calls whose one string argument is the path of what is chained after them
    pub paths: &'static [&'static str],
    /// chained calls that register their one argument, a handler, under that path, for the verb they are named after
    pub verbs: &'static [&'static str],
    /// chained calls that register a route of their own (path, handler) under that path
    pub routes: &'static [&'static str],
    /// chained calls that put the routers in their arguments under that path: Salvo `Router::with_path("api").push(..)`
    pub nests: &'static [&'static str],
}

pub static CHAINS: &[Chain] = &[
    Chain { lang: "Rust", paths: &["at", "with_path", "path", "prefix"], verbs: &["get", "post", "put", "delete", "patch"], routes: &["add"], nests: &["push"] },
];

/// how routers are chained in files of that language, if they are
pub fn chain_of(lang: &str) -> Option<&'static Chain> {
    CHAINS.iter().find(|c| c.lang == lang)
}

/// whether a method of that name, in a file of that language, answers requests
pub fn verb_method(lang: &str, name: &str) -> bool {
    VERB_METHODS.iter().any(|(l, ms)| (l.is_empty() || *l == lang) && ms.contains(&name))
}
/// a path written as a regular expression, turned into the path people see: `^articles/(?P<slug>[-\w]+)/?$` -> articles/{slug}
pub static PATTERN_PARTS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![(rx(r"\(\?P<(\w+)>[^)]*\)"), "{$1}"), (rx(r"/\?\$$"), ""), (rx(r"\$$"), ""), (rx(r"^\^"), "")]
});
/// a handler wrapped in a call that names the verb: `get(list_items)`
pub static HANDLER_WRAP: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?P<wrap>\w+)\(\s*(?P<inner>[^()]+?)\s*\)$"));

/// a class-wide route prefix: `@RequestMapping("/api")`, `@Controller('users')`, `#[Route('/api')]`
pub static CLASS_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    rx(&format!(r#"{ATTR}(?:RequestMapping|Controller|RoutePrefix|RestController|Route)\(\s*(?:value\s*=\s*|path\s*=\s*)?['"]([^'"]*)['"]"#))
});

/// files that hold nothing but route registrations: (folder, file name ending, the calls that register; none = the usual ones).
/// Their paths need no leading slash: `path("users/", view)`, `Route::get('users', ..)`
pub const ROUTE_FILES: &[(&str, &str, &[&str])] = &[
    ("", "urls.py", &["path", "re_path", "url"]),
    ("routes/", ".php", &[]),
];

/// Files whose place in the tree is the route they answer (Next.js `pages/api/users/[id].ts`, `app/api/users/[id]/route.ts`).
pub struct FileRoute {
    /// the languages it holds for (lang::Lang.name)
    pub langs: &'static [&'static str],
    /// the folder the route is read below (anywhere in the repo: `src/app/`, `web/pages/`)
    pub root: &'static str,
    /// what the path below that folder must start with to answer (`api/`: the other pages are pages, not endpoints)
    pub under: &'static str,
    /// the file name (without extension) that answers for its folder; none: every file answers, `index` for its folder
    pub stem: Option<&'static str>,
    /// the handlers are the functions exported under a verb's name (`export async function GET`), one route each;
    /// otherwise the default export answers every verb
    pub verb_exports: bool,
}

pub static FILE_ROUTES: &[FileRoute] = &[
    FileRoute { langs: JS_TS, root: "pages/", under: "api/", stem: None, verb_exports: false },
    FileRoute { langs: JS_TS, root: "app/", under: "", stem: Some("route"), verb_exports: true },
];
/// the names a file route exports its verb handlers under
pub const ROUTE_EXPORT_VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
/// a folder that is no part of the path: a route group `(marketing)`
pub static ROUTE_GROUP_DIR: LazyLock<Regex> = LazyLock::new(|| rx(r"^\(.*\)$"));
/// a folder or file that stands for a parameter: `[id]`, `[...slug]`, `[[...slug]]` -> `{id}`, `{slug}`
pub static ROUTE_PARAM_DIR: LazyLock<Regex> = LazyLock::new(|| rx(r"^\[\[?(?:\.\.\.)?(\w+)\]\]?$"));
/// the default export, by name or written in place: `export default handler`, `export default async function handler(`
pub static DEFAULT_EXPORT: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"(?m)\bexport\s+default\s+(?:(?:async\s+)?function\b\s*\*?\s*(?P<func>[\w$]*)\s*\(|(?P<name>[A-Za-z_$][\w$]*)\s*;?\s*$)")
});

/// The route a file answers by where it sits: (its file route, the path) when it is one
pub fn file_route(lang: &str, path: &str) -> Option<(&'static FileRoute, String)> {
    FILE_ROUTES.iter().filter(|r| r.langs.contains(&lang)).find_map(|r| {
        let at = if path.starts_with(r.root) { 0 } else { path.find(&format!("/{}", r.root))? + 1 };
        let rel = &path[at + r.root.len()..];
        if !rel.starts_with(r.under) {
            return None;
        }
        let (dir, file) = rel.rsplit_once('/').unwrap_or(("", rel));
        let stem = file.split('.').next().unwrap_or(file);
        let mut segs: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        match r.stem {
            Some(s) if s != stem => return None,
            Some(_) => {}
            None if stem == "index" => {}
            None => segs.push(stem),
        }
        let segs: Vec<String> = segs.into_iter().filter(|s| !ROUTE_GROUP_DIR.is_match(s)).map(|s| ROUTE_PARAM_DIR.replace(s, "{$1}").into_owned()).collect();
        Some((r, format!("/{}", segs.join("/"))))
    })
}

/// the calls that register a route in this file, if it is a routes file
pub fn routes_file(path: &str) -> Option<&'static [&'static str]> {
    ROUTE_FILES
        .iter()
        .find(|(dir, ending, _)| path.ends_with(ending) && (dir.is_empty() || path.starts_with(dir) || path.contains(&format!("/{dir}"))))
        .map(|(_, _, calls)| *calls)
}

/// a routes file handed to another routes file under a prefix: `path("api/", include("shop.urls"))`
pub static INCLUDE: LazyLock<Regex> = LazyLock::new(|| rx(r#"^include\(\s*['"]([\w.]+)['"]"#));

/// calls whose closure argument registers routes under a prefix, and where that prefix is written:
/// first argument `r.Route("/v1", func(..) {..})`, the chain before it `Route::prefix('api')->group(..)`, an array `['prefix' => 'api']`
pub const SCOPE_CALLS: &[&str] = &["Route", "group"];
pub static SCOPE_FIRST_ARG: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\(\s*['"`]([^'"`]*)['"`]"#));
pub static SCOPE_CHAIN: LazyLock<Regex> = LazyLock::new(|| rx(r#"\bprefix\s*\(\s*['"]([^'"]*)['"]\s*\)"#));
pub static SCOPE_ARRAY: LazyLock<Regex> = LazyLock::new(|| rx(r#"['"]prefix['"]\s*=>\s*['"]([^'"]*)['"]"#));

/// What an ORM looks like in code.
pub struct Orm {
    pub name: &'static str,
    /// the language it is written in (lang::Lang.name)
    pub lang: &'static str,
    /// a file that mentions one of these is written for this ORM
    pub libs: &'static [&'static str],
    /// a class that extends one of these is a model
    pub model_bases: &'static [&'static str],
    /// text above or at the start of a class that makes it a model: `#[derive(DeriveEntityModel)]`, `gorm.Model`
    pub model_attrs: &'static [&'static str],
    /// types a database session or connection is declared as
    pub handles: &'static [&'static str],
    /// names called on a model or session that read
    pub reads: &'static [&'static str],
    /// names called on a model, a session or an instance that write
    pub writes: &'static [&'static str],
    /// how generated method names start when they read: `GetUser`, `ListUsers` (`Get` then a capital letter)
    pub read_prefixes: &'static [&'static str],
    /// how generated method names start when they write: `CreateUser`, `DeleteUser`
    pub write_prefixes: &'static [&'static str],
    /// crates whose types wrap this ORM's handle: `pool: r2d2::Pool<ConnectionManager<PgConnection>>` is a Diesel session
    pub pools: &'static [&'static str],
}

/// what a MongoDB collection handle reads and writes (PyMongo, Motor)
const MONGO_READS: &[&str] = &["find", "find_one", "count_documents", "estimated_document_count", "aggregate", "distinct"];
const MONGO_WRITES: &[&str] = &["insert_one", "insert_many", "update_one", "update_many", "delete_one", "delete_many", "replace_one",
    "bulk_write", "find_one_and_update", "find_one_and_delete", "find_one_and_replace"];

pub static ORMS: &[Orm] = &[
    Orm { name: "Django ORM", lang: "Python", libs: &["django.db"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["filter", "get", "all", "first", "last", "exclude", "count", "exists", "values", "values_list", "aggregate", "annotate",
                   "select_related", "prefetch_related", "in_bulk", "latest", "earliest"],
          writes: &["save", "create", "update", "delete", "bulk_create", "bulk_update", "get_or_create", "update_or_create"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "SQLAlchemy", lang: "Python", libs: &["sqlalchemy"], model_bases: &["Base", "DeclarativeBase"], model_attrs: &[],
          handles: &["Session", "AsyncSession", "scoped_session"],
          reads: &["query", "get", "scalars", "scalar", "scalar_one", "execute", "all", "first", "one", "one_or_none", "filter", "filter_by", "count"],
          writes: &["add", "add_all", "delete", "merge", "commit", "flush", "bulk_save_objects", "bulk_insert_mappings"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "SQLModel", lang: "Python", libs: &["sqlmodel"], model_bases: &["SQLModel"], model_attrs: &[], handles: &["Session"],
          reads: &["exec", "get", "scalars", "all", "first", "one", "one_or_none"],
          writes: &["add", "add_all", "delete", "merge", "commit", "flush"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Peewee", lang: "Python", libs: &["peewee"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["select", "get", "get_or_none", "get_by_id", "where", "count", "exists", "first"],
          writes: &["create", "save", "delete_instance", "insert", "insert_many", "update", "delete", "get_or_create", "replace"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Tortoise ORM", lang: "Python", libs: &["tortoise"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["filter", "get", "all", "first", "exclude", "get_or_none", "count", "exists", "values", "values_list", "annotate"],
          writes: &["create", "save", "update", "delete", "bulk_create", "bulk_update", "get_or_create", "update_or_create"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Pony ORM", lang: "Python", libs: &["pony"], model_bases: &["Entity"], model_attrs: &[], handles: &[],
          reads: &["select", "get", "exists", "count"], writes: &["delete"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "MongoEngine", lang: "Python", libs: &["mongoengine"], model_bases: &["Document", "DynamicDocument"], model_attrs: &[], handles: &[],
          reads: &["objects", "get", "first", "all", "count", "filter", "reload"],
          writes: &["save", "delete", "update", "update_one", "insert", "modify"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Beanie", lang: "Python", libs: &["beanie"], model_bases: &["Document"], model_attrs: &[], handles: &[],
          reads: &["find", "find_one", "find_all", "find_many", "get", "aggregate", "count"],
          writes: &["insert", "insert_one", "insert_many", "save", "replace", "update", "set", "upsert", "delete", "delete_all"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "ODMantic", lang: "Python", libs: &["odmantic"], model_bases: &["Model"], model_attrs: &[], handles: &["AIOEngine", "SyncEngine", "Engine"],
          reads: &["find", "find_one", "count"], writes: &["save", "save_all", "delete", "remove"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Piccolo", lang: "Python", libs: &["piccolo"], model_bases: &["Table"], model_attrs: &[], handles: &[],
          reads: &["select", "objects", "count", "exists", "raw"], writes: &["insert", "update", "delete", "save", "remove"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "ormar", lang: "Python", libs: &["ormar"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["get", "get_or_none", "filter", "exclude", "all", "first", "count", "exists", "values", "values_list"],
          writes: &["create", "save", "update", "delete", "upsert", "bulk_create", "bulk_update", "get_or_create", "update_or_create"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // drivers: a collection or connection handle, no models
    Orm { name: "PyMongo", lang: "Python", libs: &["pymongo"], model_bases: &[], model_attrs: &[], handles: &["Collection", "Database", "MongoClient"],
          reads: MONGO_READS, writes: MONGO_WRITES, read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Motor", lang: "Python", libs: &["motor"], model_bases: &[], model_attrs: &[],
          handles: &["AsyncIOMotorCollection", "AsyncIOMotorDatabase", "AsyncIOMotorClient"],
          reads: MONGO_READS, writes: MONGO_WRITES, read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "asyncpg", lang: "Python", libs: &["asyncpg"], model_bases: &[], model_attrs: &[], handles: &["Connection", "Pool"],
          reads: &["fetch", "fetchrow", "fetchval"], writes: &["execute", "executemany"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // the SQL text is not read: execute counts as a write, fetching its rows as a read
    Orm { name: "psycopg", lang: "Python", libs: &["psycopg"], model_bases: &[], model_attrs: &[],
          handles: &["Connection", "Cursor", "AsyncConnection", "AsyncCursor"],
          reads: &["fetchone", "fetchall", "fetchmany"], writes: &["execute", "executemany"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "GORM", lang: "Go", libs: &["gorm.io", "jinzhu/gorm"], model_bases: &[], model_attrs: &["gorm.Model"], handles: &["gorm.DB"],
          reads: &["Find", "First", "Take", "Last", "Where", "Scan", "Pluck", "Count", "Preload", "Joins", "Rows", "Row"],
          writes: &["Create", "Save", "Update", "Updates", "UpdateColumn", "Delete", "Exec", "FirstOrCreate", "CreateInBatches"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "sqlx", lang: "Go", libs: &["jmoiron/sqlx"], model_bases: &[], model_attrs: &[], handles: &["sqlx.DB", "sqlx.Tx", "sqlx.Conn"],
          reads: &["Get", "Select", "Query", "Queryx", "QueryRow", "QueryRowx", "QueryContext", "QueryRowContext", "GetContext", "SelectContext", "NamedQuery"],
          writes: &["Exec", "ExecContext", "MustExec", "NamedExec", "NamedExecContext"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "database/sql", lang: "Go", libs: &["database/sql"], model_bases: &[], model_attrs: &[], handles: &["sql.DB", "sql.Tx", "sql.Conn"],
          reads: &["Query", "QueryRow", "QueryContext", "QueryRowContext"], writes: &["Exec", "ExecContext"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "ent", lang: "Go", libs: &["entgo.io"], model_bases: &[], model_attrs: &[], handles: &["ent.Client", "ent.Tx"],
          reads: &["Query", "All", "Only", "First", "Count", "Exist", "Get", "IDs"],
          writes: &["Create", "Update", "UpdateOne", "Delete", "DeleteOne", "Save", "Exec"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Bun", lang: "Go", libs: &["uptrace/bun"], model_bases: &[], model_attrs: &[], handles: &["bun.DB", "bun.Tx"],
          reads: &["NewSelect", "Scan", "ScanAndCount", "Count", "Exists"], writes: &["NewInsert", "NewUpdate", "NewDelete", "Exec"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // generated code: the method names are the queries the developer named
    Orm { name: "sqlc", lang: "Go", libs: &["Queries"], model_bases: &[], model_attrs: &[], handles: &["Queries"], reads: &[], writes: &[],
          read_prefixes: &["Get", "List", "Find", "Count", "Search"], write_prefixes: &["Create", "Update", "Delete", "Insert", "Upsert"], pools: &[] },
    Orm { name: "pgx", lang: "Go", libs: &["jackc/pgx"], model_bases: &[], model_attrs: &[], handles: &["pgxpool.Pool", "pgx.Conn", "pgx.Tx"],
          reads: &["Query", "QueryRow"], writes: &["Exec", "SendBatch", "CopyFrom"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "xorm", lang: "Go", libs: &["xorm.io", "go-xorm/xorm"], model_bases: &[], model_attrs: &["xorm:\""], handles: &["xorm.Engine", "xorm.Session"],
          reads: &["Get", "Find", "FindAndCount", "Count", "Exist", "Iterate", "Rows", "Sum", "Query", "QueryString", "QueryInterface", "Where", "SQL"],
          writes: &["Insert", "InsertOne", "Update", "Delete", "Exec", "Commit"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Beego ORM", lang: "Go", libs: &["beego/v2/client/orm", "astaxie/beego/orm"], model_bases: &[], model_attrs: &[],
          handles: &["orm.Ormer", "orm.NewOrm"],
          reads: &["Read", "ReadOrCreate", "ReadForUpdate", "QueryTable", "Raw", "LoadRelated"],
          writes: &["Insert", "InsertMulti", "InsertOrUpdate", "Update", "Delete", "Exec"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "SQLx", lang: "Rust", libs: &["sqlx"], model_bases: &[], model_attrs: &[],
          handles: &["PgPool", "MySqlPool", "SqlitePool", "PgConnection", "MySqlConnection", "SqliteConnection", "Pool", "Transaction"],
          reads: &["fetch_all", "fetch_one", "fetch_optional", "fetch"], writes: &["execute"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Diesel", lang: "Rust", libs: &["diesel"], model_bases: &[], model_attrs: &["Queryable", "Insertable", "Identifiable", "AsChangeset"],
          handles: &["PgConnection", "MysqlConnection", "SqliteConnection", "Pool"],
          reads: &["load", "first", "get_result", "get_results"], writes: &["execute"], read_prefixes: &[], write_prefixes: &[],
          pools: &["r2d2", "deadpool_diesel", "diesel_async", "bb8", "deadpool"] },
    Orm { name: "SeaORM", lang: "Rust", libs: &["sea_orm"], model_bases: &[], model_attrs: &["DeriveEntityModel"],
          handles: &["DatabaseConnection", "DatabaseTransaction"],
          reads: &["all", "one", "count", "paginate"], writes: &["insert", "update", "delete", "save", "exec"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // the SQL text is not read: execute counts as a write, a query (or preparing one) as a read
    Orm { name: "rusqlite", lang: "Rust", libs: &["rusqlite"], model_bases: &[], model_attrs: &[], handles: &["Connection", "Transaction"],
          reads: &["query_row", "prepare", "prepare_cached"], writes: &["execute", "execute_batch"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "tokio-postgres", lang: "Rust", libs: &["tokio_postgres"], model_bases: &[], model_attrs: &[], handles: &["Client", "Transaction"],
          reads: &["query", "query_one", "query_opt", "query_raw"], writes: &["execute", "batch_execute", "execute_raw"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // the functions crud! / impl_select! give a model take the handle: `Activity::select_all(&rb)`
    Orm { name: "Rbatis", lang: "Rust", libs: &["rbatis"], model_bases: &[], model_attrs: &[], handles: &["RBatis", "Rbatis"],
          reads: &["query_decode", "query", "select_all", "select_by_map", "select_by_column", "select_in_column", "select_page"],
          writes: &["exec", "insert", "insert_batch", "update_by_map", "update_by_column", "delete_by_map", "delete_by_column", "delete_in_column"],
          read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "MongoDB", lang: "Rust", libs: &["mongodb"], model_bases: &[], model_attrs: &[], handles: &["Collection"],
          reads: MONGO_READS, writes: MONGO_WRITES, read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Eloquent", lang: "PHP", libs: &["Illuminate\\Database\\Eloquent"], model_bases: &["Model", "Authenticatable", "Pivot"], model_attrs: &[], handles: &[],
          reads: &["where", "find", "first", "get", "all", "count", "pluck", "findOrFail", "firstOrFail", "paginate", "with", "orderBy", "exists", "select"],
          writes: &["create", "save", "update", "delete", "destroy", "insert", "updateOrCreate", "firstOrCreate", "forceDelete"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Doctrine", lang: "PHP", libs: &["Doctrine\\ORM"], model_bases: &[],
          model_attrs: &["#[ORM\\Entity", "@ORM\\Entity", "@Entity"], handles: &["EntityManagerInterface", "EntityManager", "ObjectManager"],
          reads: &["find", "findAll", "findBy", "findOneBy", "createQueryBuilder", "getResult", "getOneOrNullResult", "getSingleResult", "createQuery"],
          writes: &["persist", "remove", "flush", "merge"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "PDO", lang: "PHP", libs: &["PDO"], model_bases: &[], model_attrs: &[], handles: &["PDO"], reads: &["query", "prepare"], writes: &["exec"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // JavaScript, and TypeScript too (ORM_LANGS_ALSO)
    // the model is the member the call is made through: `prisma.user.findMany()` reads User (ORM_FORMS)
    Orm { name: "Prisma", lang: "JavaScript", libs: &["@prisma/client"], model_bases: &[], model_attrs: &[], handles: &["PrismaClient"],
          reads: &["findMany", "findUnique", "findUniqueOrThrow", "findFirst", "findFirstOrThrow", "count", "aggregate", "groupBy", "$queryRaw", "$queryRawUnsafe"],
          writes: &["create", "createMany", "update", "updateMany", "upsert", "delete", "deleteMany", "$executeRaw", "$executeRawUnsafe"],
          read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // a repository names its entity where it is made (`getRepository(User)`), not where it is declared (`Repository<User>`)
    Orm { name: "TypeORM", lang: "JavaScript", libs: &["typeorm"], model_bases: &["BaseEntity"], model_attrs: &["@Entity"],
          handles: &["Repository", "EntityManager", "DataSource", "getRepository"],
          reads: &["find", "findOne", "findBy", "findOneBy", "count", "exists", "getMany", "getOne", "getCount"],
          writes: &["save", "insert", "update", "delete", "remove", "softDelete"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // models are classes or made by a call (ORM_FORMS): `const Post = sequelize.define('Post', {..})`
    Orm { name: "Sequelize", lang: "JavaScript", libs: &["sequelize"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["findAll", "findOne", "findByPk", "count"], writes: &["create", "bulkCreate", "update", "destroy", "upsert", "save"],
          read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "Mongoose", lang: "JavaScript", libs: &["mongoose"], model_bases: &[], model_attrs: &[], handles: &[],
          reads: &["find", "findOne", "findById", "countDocuments", "aggregate"],
          writes: &["create", "insertMany", "updateOne", "updateMany", "findByIdAndUpdate", "findOneAndUpdate", "deleteOne", "deleteMany",
                    "findByIdAndDelete", "save"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // tables are values (`pgTable('users', ..)`), not classes: the steps say database
    Orm { name: "Drizzle", lang: "JavaScript", libs: &["drizzle-orm"], model_bases: &[], model_attrs: &[], handles: &["drizzle"],
          reads: &["select", "findMany", "findFirst"], writes: &["insert", "update", "delete"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    // the handle is called with the table first (ORM_FORMS): `db('users').where(..).first()`; the SQL of `raw` is not read
    Orm { name: "Knex", lang: "JavaScript", libs: &["knex"], model_bases: &[], model_attrs: &[], handles: &["knex"],
          reads: &["select", "where", "first", "pluck"], writes: &["insert", "update", "del", "delete"], read_prefixes: &[], write_prefixes: &[], pools: &[] },
    Orm { name: "MikroORM", lang: "JavaScript", libs: &["@mikro-orm"], model_bases: &[], model_attrs: &["@Entity"], handles: &["EntityManager"],
          reads: &["find", "findOne", "count"], writes: &["persist", "persistAndFlush", "remove", "flush", "nativeInsert", "nativeUpdate", "nativeDelete"],
          read_prefixes: &[], write_prefixes: &[], pools: &[] },
];

/// an ORM written for the first language is used from files of the second too
pub const ORM_LANGS_ALSO: &[(&str, &str)] = &[("JavaScript", "TypeScript")];

/// whether an ORM written for `orm` is used from files of language `file`
pub fn orm_speaks(orm: &str, file: &str) -> bool {
    orm == file || ORM_LANGS_ALSO.iter().any(|(o, f)| *o == orm && *f == file)
}

/// languages whose ORM code is written the JavaScript way: a handle made by `new` (`prisma = new PrismaClient()`),
/// an awaited model call that hands back an instance (`user = await User.findByPk(1)`), members named with `$` (`prisma.$queryRaw`)
pub const ORM_JS_SYNTAX: &[&str] = JS_TS;

/// How some ORMs are written beyond what Orm says
pub struct OrmForm {
    /// Orm.name
    pub orm: &'static str,
    /// calls that make a model, named by the variable it is put in or else by its first argument:
    /// `const Post = sequelize.define('Post', ..)`, `module.exports = mongoose.model('User', schema)`
    pub model_calls: &'static [&'static str],
    /// the member a call is made through on the handle is the model, spelt with a capital: `prisma.orderItem.count()` reads OrderItem
    pub member_models: bool,
    /// the handle is called before the chain: `db('users').where(..).first()`
    pub called_handle: bool,
}

pub static ORM_FORMS: &[OrmForm] = &[
    OrmForm { orm: "Prisma", model_calls: &[], member_models: true, called_handle: false },
    OrmForm { orm: "Sequelize", model_calls: &["define"], member_models: false, called_handle: false },
    OrmForm { orm: "Mongoose", model_calls: &["model"], member_models: false, called_handle: false },
    OrmForm { orm: "Knex", model_calls: &[], member_models: false, called_handle: true },
];

/// how the ORM of that name is written beyond Orm, if it says more
pub fn orm_form(orm: &str) -> Option<&'static OrmForm> {
    ORM_FORMS.iter().find(|f| f.orm == orm)
}
