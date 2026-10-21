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
/// a route written above a function: @app.get("/x"), @router.post(...), @GetMapping("/x"), [HttpGet("x")]
pub static DECORATOR_ROUTES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        rx(r#"@(?:[\w.]+\.)?(?i:(get|post|put|delete|patch|route|head|options|websocket))\(\s*[rbuf]*['"]([^'"]+)['"]"#),
        rx(r#"@(Get|Post|Put|Delete|Patch|Request)Mapping\(\s*(?:value\s*=\s*|path\s*=\s*)?"([^"]*)""#),
        rx(r#"\[Http(Get|Post|Put|Delete|Patch)\(\s*"([^"]*)""#),
    ]
});
/// call names that register a web endpoint: app.get("/x", handler), mux.HandleFunc("/x", h)
pub const ROUTE_CALLS: &[&str] = &["get", "post", "put", "delete", "patch", "all", "any", "route", "handle", "HandleFunc", "Handle",
    "GET", "POST", "PUT", "DELETE", "PATCH"];
