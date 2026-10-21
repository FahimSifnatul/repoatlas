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
    "GET", "POST", "PUT", "DELETE", "PATCH"];

/// how a route's handler is written, the last argument of the registering call: the class (if any) and the function.
/// `[UserController::class, 'index']`, `'UserController@index'`, `ctrl.profile`, `list_items`
pub static HANDLER_FORMS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        rx(r#"^\[\s*(?:[\w\\]+\\)?(?P<class>\w+)(?:::class|\.class)?\s*,\s*['"](?P<func>\w+)['"]\s*\]$"#),
        rx(r#"^['"](?:[\w\\]+\\)?(?P<class>\w+)@(?P<func>\w+)['"]$"#),
        rx(r"^(?:[\w$]+(?:\.|::))*(?P<class>[\w$]+)(?:\.|::)(?P<func>[\w$]+)$"),
        rx(r"^(?P<func>[\w$]+)$"),
    ]
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
