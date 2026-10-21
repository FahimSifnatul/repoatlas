//! Data only: how web frameworks write routes and mount them on each other.
//! Framework names may appear here and in tables.rs, never in the logic (a test checks that).

use std::sync::LazyLock;

use regex::Regex;

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

/// calls that put one router under another
pub const MOUNT_CALLS: &[&str] = &["include_router", "register_blueprint", "use", "mount", "Mount", "add_router"];
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
    "add_url_rule", "add_get", "add_post", "add_put", "add_delete", "add_patch"];
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
        rx(r#"^\[\s*(?:[\w\\]+\\)?(?P<class>\w+)(?:::class|\.class)?\s*,\s*['"](?P<func>\w+)['"]\s*\]$"#),
        rx(r#"^['"](?:[\w\\]+\\)?(?P<class>\w+)@(?P<func>\w+)['"]$"#),
        rx(r"^(?:[\w$]+(?:\.|::))*(?P<class>[\w$]+)(?:\.|::)(?P<func>[\w$]+)$"),
        rx(r"^(?P<func>[\w$]+)$"),
    ]
});
/// the methods of a class-based view or viewset that answer requests
pub const VERB_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "list", "create", "retrieve", "update", "partial_update", "destroy"];
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
}

pub static ORMS: &[Orm] = &[
    Orm { name: "Django ORM", lang: "Python", libs: &["django.db"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["filter", "get", "all", "first", "last", "exclude", "count", "exists", "values", "values_list", "aggregate", "annotate",
                   "select_related", "prefetch_related", "in_bulk", "latest", "earliest"],
          writes: &["save", "create", "update", "delete", "bulk_create", "bulk_update", "get_or_create", "update_or_create"] },
    Orm { name: "SQLAlchemy", lang: "Python", libs: &["sqlalchemy"], model_bases: &["Base", "DeclarativeBase"], model_attrs: &[],
          handles: &["Session", "AsyncSession", "scoped_session"],
          reads: &["query", "get", "scalars", "scalar", "scalar_one", "execute", "all", "first", "one", "one_or_none", "filter", "filter_by", "count"],
          writes: &["add", "add_all", "delete", "merge", "commit", "flush", "bulk_save_objects", "bulk_insert_mappings"] },
    Orm { name: "SQLModel", lang: "Python", libs: &["sqlmodel"], model_bases: &["SQLModel"], model_attrs: &[], handles: &["Session"],
          reads: &["exec", "get", "scalars", "all", "first", "one", "one_or_none"],
          writes: &["add", "add_all", "delete", "merge", "commit", "flush"] },
    Orm { name: "Peewee", lang: "Python", libs: &["peewee"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["select", "get", "get_or_none", "get_by_id", "where", "count", "exists", "first"],
          writes: &["create", "save", "delete_instance", "insert", "insert_many", "update", "delete", "get_or_create", "replace"] },
    Orm { name: "Tortoise ORM", lang: "Python", libs: &["tortoise"], model_bases: &["Model"], model_attrs: &[], handles: &[],
          reads: &["filter", "get", "all", "first", "exclude", "get_or_none", "count", "exists", "values", "values_list", "annotate"],
          writes: &["create", "save", "update", "delete", "bulk_create", "bulk_update", "get_or_create", "update_or_create"] },
    Orm { name: "Pony ORM", lang: "Python", libs: &["pony"], model_bases: &["Entity"], model_attrs: &[], handles: &[],
          reads: &["select", "get", "exists", "count"], writes: &["delete"] },
    Orm { name: "GORM", lang: "Go", libs: &["gorm.io", "jinzhu/gorm"], model_bases: &[], model_attrs: &["gorm.Model"], handles: &["gorm.DB"],
          reads: &["Find", "First", "Take", "Last", "Where", "Scan", "Pluck", "Count", "Preload", "Joins", "Rows", "Row"],
          writes: &["Create", "Save", "Update", "Updates", "UpdateColumn", "Delete", "Exec", "FirstOrCreate", "CreateInBatches"] },
    Orm { name: "sqlx", lang: "Go", libs: &["jmoiron/sqlx"], model_bases: &[], model_attrs: &[], handles: &["sqlx.DB", "sqlx.Tx", "sqlx.Conn"],
          reads: &["Get", "Select", "Query", "Queryx", "QueryRow", "QueryRowx", "QueryContext", "QueryRowContext", "GetContext", "SelectContext", "NamedQuery"],
          writes: &["Exec", "ExecContext", "MustExec", "NamedExec", "NamedExecContext"] },
    Orm { name: "database/sql", lang: "Go", libs: &["database/sql"], model_bases: &[], model_attrs: &[], handles: &["sql.DB", "sql.Tx", "sql.Conn"],
          reads: &["Query", "QueryRow", "QueryContext", "QueryRowContext"], writes: &["Exec", "ExecContext"] },
    Orm { name: "ent", lang: "Go", libs: &["entgo.io"], model_bases: &[], model_attrs: &[], handles: &["ent.Client", "ent.Tx"],
          reads: &["Query", "All", "Only", "First", "Count", "Exist", "Get", "IDs"],
          writes: &["Create", "Update", "UpdateOne", "Delete", "DeleteOne", "Save", "Exec"] },
    Orm { name: "Bun", lang: "Go", libs: &["uptrace/bun"], model_bases: &[], model_attrs: &[], handles: &["bun.DB", "bun.Tx"],
          reads: &["NewSelect", "Scan", "ScanAndCount", "Count", "Exists"], writes: &["NewInsert", "NewUpdate", "NewDelete", "Exec"] },
    Orm { name: "SQLx", lang: "Rust", libs: &["sqlx"], model_bases: &[], model_attrs: &[],
          handles: &["PgPool", "MySqlPool", "SqlitePool", "PgConnection", "MySqlConnection", "SqliteConnection", "Pool", "Transaction"],
          reads: &["fetch_all", "fetch_one", "fetch_optional", "fetch"], writes: &["execute"] },
    Orm { name: "Diesel", lang: "Rust", libs: &["diesel"], model_bases: &[], model_attrs: &["Queryable", "Insertable", "Identifiable", "AsChangeset"],
          handles: &["PgConnection", "MysqlConnection", "SqliteConnection", "Pool"],
          reads: &["load", "first", "get_result", "get_results"], writes: &["execute"] },
    Orm { name: "SeaORM", lang: "Rust", libs: &["sea_orm"], model_bases: &[], model_attrs: &["DeriveEntityModel"],
          handles: &["DatabaseConnection", "DatabaseTransaction"],
          reads: &["all", "one", "count", "paginate"], writes: &["insert", "update", "delete", "save", "exec"] },
    Orm { name: "Eloquent", lang: "PHP", libs: &["Illuminate\\Database\\Eloquent"], model_bases: &["Model", "Authenticatable", "Pivot"], model_attrs: &[], handles: &[],
          reads: &["where", "find", "first", "get", "all", "count", "pluck", "findOrFail", "firstOrFail", "paginate", "with", "orderBy", "exists", "select"],
          writes: &["create", "save", "update", "delete", "destroy", "insert", "updateOrCreate", "firstOrCreate", "forceDelete"] },
    Orm { name: "Doctrine", lang: "PHP", libs: &["Doctrine\\ORM"], model_bases: &[],
          model_attrs: &["#[ORM\\Entity", "@ORM\\Entity", "@Entity"], handles: &["EntityManagerInterface", "EntityManager", "ObjectManager"],
          reads: &["find", "findAll", "findBy", "findOneBy", "createQueryBuilder", "getResult", "getOneOrNullResult", "getSingleResult", "createQuery"],
          writes: &["persist", "remove", "flush", "merge"] },
    Orm { name: "PDO", lang: "PHP", libs: &["PDO"], model_bases: &[], model_attrs: &[], handles: &["PDO"], reads: &["query", "prepare"], writes: &["exec"] },
];
