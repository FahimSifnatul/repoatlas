//! Data only: layers, file-name words, library catalogue, launch files, inputs and outputs.
//! Framework and library names may appear here and in lang.rs, never in the logic (a test checks that).

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

pub const SKIP_DIRS: &[&str] = &[
    "node_modules", "vendor", "third_party", "site-packages", "dist", "build", "out", "target", "obj", "__pycache__",
    "venv", "virtualenv", "coverage", "htmlcov", "docs", "documentation", "site", "storybook-static",
];
pub const MAX_BYTES: u64 = 400_000;

/// key, display name, plain-English tagline, colour, group
pub const LAYERS: &[(&str, &str, &str, &str, &str)] = &[
    ("interface", "Interface", "What people see and touch: screens, pages and commands.", "#e0457b", "stack"),
    ("api", "API & Routing", "The front door: receives requests and sends them to the right place.", "#f08a24", "stack"),
    ("logic", "Core Logic", "The brain: the rules and steps that make things happen.", "#4f5bd5", "stack"),
    ("data", "Data & Storage", "The memory: models, databases and saved information.", "#1fa971", "stack"),
    ("integration", "Integrations", "The phone lines: talks to outside services.", "#17a2b8", "stack"),
    ("shared", "Shared Toolkit", "Small helpers that the rest of the project reuses.", "#8e6bd8", "stack"),
    ("config", "Config & Tooling", "Settings, build scripts and deployment.", "#6b7a90", "support"),
    ("tests", "Tests & Examples", "Checks that it works, and samples of how to use it.", "#c9961a", "support"),
];

pub fn short(layer: &str) -> &'static str {
    match layer {
        "interface" => "Screens and commands people interact with",
        "api" => "Receives requests and routes them",
        "logic" => "Core rules and processes",
        "data" => "Data models and storage",
        "integration" => "Talks to outside services",
        "shared" => "Reusable helper code",
        "config" => "Settings and tooling",
        "tests" => "Tests and examples",
        _ => "",
    }
}

/// words in file / folder names that reveal a file's role (first layer listed wins on conflicts)
const TOKENS: &[(&str, &str)] = &[
    ("tests", "test tests spec specs e2e testing fixtures mocks mock cypress playwright example examples conftest factories \
               demo demos sample samples benchmark benchmarks bench"),
    ("config", "config configs conf settings setup env deploy deployment infra infrastructure terraform k8s \
                kubernetes helm docker ci scripts script build webpack vite rollup babel eslint jest tailwind \
                postcss gulpfile gruntfile packaging release releases install installer homebrew formula doc docs documentation \
                lint format typecheck generate codegen"),
    ("interface", "ui components component pages page views view templates template static public assets \
                   frontend web screens screen layouts layout widgets widget styles style css cli cmd commands \
                   command console gui tui forms form theme themes admin"),
    ("api", "api apis routes route router routers controllers controller handlers handler endpoints endpoint \
             resolvers resolver graphql rest grpc middleware middlewares server urls rpc webhook webhooks \
             serializers serializer wsgi asgi"),
    ("logic", "services service core domain usecases usecase logic engine managers manager business workflows \
               workflow tasks task jobs job workers worker processors processor pipeline pipelines signals"),
    ("data", "models model db database databases repository repositories repo repos dao entities entity schema \
              schemas migrations migration orm store stores storage prisma sql queries query dal persistence cache"),
    ("integration", "clients client adapters adapter integrations integration gateway gateways connectors \
                     connector sdk drivers driver external"),
    ("shared", "utils util utilities helpers helper common shared support misc constants types typing compat"),
];
/// deep in a source tree these name a package (org/acme/samples/shop), not a folder of examples
pub const WEAK_TESTS: &[&str] = &["example", "examples", "demo", "demos", "sample", "samples", "benchmark", "benchmarks", "bench", "fixtures"];

pub static TOKEN_LAYER: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    for (layer, words) in TOKENS {
        for w in words.split_whitespace() {
            m.entry(w).or_insert(*layer);
        }
    }
    m
});

/// well-known libraries -> what kind of thing they are (used for layer hints and the "Built with" list)
pub const EXT_CATS: &[(&str, &str, &str)] = &[
    ("web", "Web framework", "flask fastapi django starlette aiohttp tornado sanic bottle werkzeug gunicorn uvicorn express koa \
        rails sinatra hanami puma laravel/framework laravel/lumen-framework symfony/framework-bundle slim/slim \
        spring-boot-starter-web spring-boot-starter-webflux ktor-server-core \
        fastify hapi next nuxt @nestjs/core @nestjs/common github.com/gin-gonic/gin github.com/gorilla/mux \
        github.com/labstack/echo github.com/gofiber/fiber net/http org.springframework.web \
        org.springframework.boot actix_web axum rocket warp"),
    ("db", "Database", "sqlalchemy sqlmodel alembic psycopg2 psycopg asyncpg pymysql pymongo motor peewee tortoise sqlite3 mongoose \
        mongodb sequelize typeorm prisma @prisma/client knex pg mysql mysql2 better-sqlite3 redis ioredis \
        gorm.io/gorm github.com/jmoiron/sqlx database/sql github.com/redis/go-redis github.com/lib/pq \
        github.com/jackc/pgx org.hibernate javax.persistence jakarta.persistence org.springframework.data \
        diesel sqlx rusqlite elasticsearch cassandra"),
    ("ui", "UI framework", "react react-dom vue svelte @angular/core preact solid-js lit tkinter pyqt5 pyqt6 \
        pyside2 pyside6 streamlit gradio kivy jquery @mui/material antd"),
    ("queue", "Messaging & jobs", "celery rq dramatiq kafka confluent_kafka kafkajs amqplib pika bull bullmq nats"),
    ("cloud", "Cloud & SaaS", "boto3 botocore aws-sdk @aws-sdk google.cloud @google-cloud firebase firebase-admin \
        supabase azure stripe twilio sendgrid slack_sdk @slack"),
    ("http", "HTTP clients", "requests httpx urllib3 axios node-fetch got superagent reqwest okhttp retrofit"),
    ("ai", "AI & data science", "openai anthropic langchain transformers torch tensorflow sklearn keras numpy \
        pandas scipy xgboost"),
];
pub static EXT_CAT: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| EXT_CATS.iter().flat_map(|(cat, _, words)| words.split_whitespace().map(move |w| (w, *cat))).collect());
pub const CAT_LAYER: &[(&str, &str)] =
    &[("ui", "interface"), ("web", "api"), ("db", "data"), ("http", "integration"), ("cloud", "integration"), ("queue", "integration")];
/// words that run the command after them: `exec gunicorn app.wsgi` is gunicorn
pub const WRAPPERS: &[&str] = &["exec", "env", "nohup", "sudo", "time", "command"];

pub const ENTRY_STEMS: &[&str] = &["main", "__main__", "manage", "wsgi", "asgi", "server", "app", "index", "cli", "run", "start", "program"];

/// files that write down how the project is started (read by launch::read_launch_files)
pub static LAUNCH_FILES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)^(?:(?:.+\.)?Dockerfile(?:\..+)?|.+\.dockerfile|.+\.docker)$", "docker"),
        (r"^Procfile(?:\..+)?$", "procfile"),
        (r"^.+\.ya?ml$", "yaml"),                 // compose, k8s, helm, CI, serverless
        (r"^.+\.json$", "json"),                  // package / composer / deno scripts
        (r"^.+\.toml$", "toml"),                  // [project.scripts] and friends
        (r"^.+\.(?:service|conf|ini)$", "ini"),   // systemd, supervisor, nginx / Apache
        (r"^(?:GNU)?[Mm]akefile$", "make"),       // last: a Makefile mostly wraps the commands above
    ]
    .into_iter()
    .map(|(p, k)| (Regex::new(p).unwrap(), k))
    .collect()
});
/// files that list a project's dependencies: a tool named there is the project's own framework (next dev, rails s)
pub static DEP_FILES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:package\.json|composer\.json|Gemfile|pyproject\.toml|requirements[\w.-]*\.txt|Pipfile|setup\.(?:py|cfg)|go\.mod|Cargo\.toml|pom\.xml|build\.gradle(?:\.kts)?|.+\.csproj|mix\.exs|pubspec\.yaml|deno\.json)$").unwrap()
});

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap()
}
fn frx(p: &str) -> fancy_regex::Regex {
    fancy_regex::Regex::new(p).unwrap()
}

pub static CHDIR: LazyLock<Regex> = LazyLock::new(|| rx(r#"\b(?:chdir|Chdir|cd)\s*\(?\s*["']?(/?[\w./-]+)"#));
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

/// comments and docstrings mention files and calls the code never makes; long strings hold messages, patterns, SQL
/// (used only for files without a grammar: pages and style sheets)
pub static COMMENTS: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    frx(r#"(?m)^[ \t]*[rRbBuU]{0,2}("""|''')[\s\S]*?\1|/\*[\s\S]*?\*/|(?:^|(?<=\s))(?:#|//)(?=\s)[^\n]*|<!--[\s\S]*?-->"#)
});
pub static STRINGS: LazyLock<fancy_regex::Regex> =
    LazyLock::new(|| frx(r#"("""|''')[\s\S]*?\1|"(?:[^"\\\n]|\\.){4,}"|'(?:[^'\\\n]|\\.){4,}'"#));
/// a file path written anywhere in a file: "viewer.html", ./lib.sh, templates/page.html, __DIR__."/../routes/web.php"
pub static PATH_REF: LazyLock<fancy_regex::Regex> =
    LazyLock::new(|| frx(r"(?<![\w/.-])(/?(?:\.{1,2}/)*[\w-][\w.-]*(?:/[\w.-]+)*\.[A-Za-z]\w{0,7})(?![\w/-])"));
pub static STRING: LazyLock<Regex> = LazyLock::new(|| rx(r#""((?:[^"\\\n]|\\.)+)"|'((?:[^'\\\n]|\\.)+)'|`([^`\n]+)`"#));
pub static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| rx(r"%[-+\d.]*[sdvif]|\$?\{[^{}]*\}|\*+"));
pub static MODULE: LazyLock<Regex> = LazyLock::new(|| rx(r"^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+$"));
/// what a module name leaves out of its file path
pub static STEM: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:/(?:__init__|index|mod))?\.\w+$"));
/// names too common to say which file is meant
pub const COMMON_NAMES: &[&str] = &[
    "main", "init", "new", "New", "run", "Run", "get", "set", "test", "setup", "start", "stop", "close", "Close", "String",
    "Error", "error", "handle", "handler", "index", "update", "create", "delete", "render", "apply", "call", "parse", "load",
    "save", "build",
];
/// method names every language's standard types have: obj.get() on an unknown object is not the repo's get()
pub const COMMON_METHODS: &[&str] = &[
    "append", "extend", "get", "set", "items", "keys", "values", "join", "split", "format", "strip", "replace", "read", "write",
    "open", "close", "add", "update", "pop", "push", "map", "filter", "reduce", "forEach", "then", "catch", "finally",
    "toString", "equals", "hashCode", "length", "size", "put", "remove", "insert", "find", "sort", "copy", "clear", "startswith",
    "endswith", "lower", "upper", "encode", "decode", "match", "search", "sub", "exec", "test", "call", "apply", "bind",
    "emit", "on", "send", "log", "error", "info", "warn", "debug", "print", "println", "printf", "String", "Error", "len",
    "contains", "includes", "indexOf", "slice", "splice", "concat", "trim", "count", "next", "iter", "unwrap", "expect",
    "clone", "into", "to_string", "as_str", "collect", "is_empty", "new", "default", "lock", "wait", "run", "start", "stop",
    "setdefault", "group", "groups", "rstrip", "lstrip", "splitlines", "readline", "exists", "resolve", "with", "has",
    "delete", "create", "parse", "load", "save", "dump", "dumps", "loads", "list", "dict", "tuple", "sum", "min", "max",
];

pub struct Io {
    pub key: &'static str,
    pub label: &'static str,
    pub way: &'static str,
    pattern: Option<&'static str>,
}

/// where data comes in and goes out: key, plain-English label, direction, pattern in the source (any language)
pub const IO: &[Io] = &[
    Io { key: "args", label: "Command-line arguments", way: "in", pattern: Some(
        r#"\bsys\.argv\b|\bargparse\b|\bprocess\.argv\b|\bos\.Args\b|\bflag\.Parse\(|\bclap::|\bcobra\.Command\b|\bcommander\b|\byargs\b|\bARGV\b|\$argv\b|CommandLine\.arguments|\bString\[\]\s*args\b|\bgetopts?\b|"\$[@1]"|\benv::args\b"#) },
    Io { key: "env", label: "Environment variables", way: "in", pattern: Some(
        r"\bos\.environ\b|\b[Gg]etenv\b|\bprocess\.env\b|\benv::var\b|\bENV\[|GetEnvironmentVariable\(|\bdotenv\b|\$\{[A-Z][A-Z0-9_]*:?[-=]") },
    Io { key: "stdin", label: "Keyboard input", way: "in", pattern: Some(
        r"(?:^|[^\w.>:])input\(|\bsys\.stdin\b|\bprocess\.stdin\b|\bos\.Stdin\b|\bSystem\.in\b|\bscanf\(|\bConsole\.ReadLine\b|\breadLine\(|^\s*read\s+-r\b|\$stdin\b|\bSTDIN\b|\bio::stdin\b") },
    Io { key: "http_in", label: "Web requests", way: "in", pattern: Some(
        r"\.listen\(|ListenAndServe|\bapp\.run\(|\buvicorn\.run\(|\bserve_forever\(|\bcreateServer\(|SpringApplication\.run|\bHttpServer\b|\baxum::serve\b|WebApplication\.Create") },
    Io { key: "file_in", label: "Files", way: "in", pattern: Some(
        r"(?:^|[^\w.>:])open\(|\bread_text\(|\bread_bytes\(|\breadFile(?:Sync)?\(|\bReadFile\(|\bos\.Open\(|\bFile\.read|\bfs::read|\bfopen\(|\bFiles\.read|\bFile\.ReadAll|\bos\.walk\(|\breaddir(?:Sync)?\(|\bReadDir\(|\bget_data\(|\bFile::open\(|\bWalkBuilder\b") },
    Io { key: "http_resp", label: "Web responses", way: "out", pattern: None },   // whatever serves web requests answers them
    Io { key: "stdout", label: "Terminal output", way: "out", pattern: Some(
        r"(?:^|[^\w.>:])print\(|\bfmt\.(?:F?Print|Fprint)|\bprintln!|\beprintln!|\bSystem\.out\.print|(?:^|[^\w.>:])(?:printf|puts)\(|\bConsole\.Write|\bprocess\.stdout\b|\bsys\.stdout\b|^\s*(?:puts|echo|printf)\s|\bconsole\.log\(") },
    Io { key: "file_out", label: "Files", way: "out", pattern: Some(
        r#"\bwrite_text\(|\bwrite_bytes\(|\bwriteFile(?:Sync)?\(|\bWriteFile\(|\bos\.Create\(|\bFile\.write|\bfs::write|\bFiles\.write|\bFile\.WriteAll|(?:^|[^\w.>:])open\([^)\n]*['"][wax]b?\+?['"]|\bfopen\([^)\n]*"[wa]|\bcreateWriteStream\(|\bFile::create\("#) },
    Io { key: "http_out", label: "Web service calls", way: "out", pattern: Some(
        r"\brequests\.(?:get|post|put|delete|patch|request)\(|\bhttpx\.|\burlopen\(|(?:^|[^\w.>:])fetch\(|\baxios\b|\bhttp\.(?:Get|Post|NewRequest)\(|\breqwest::|\bHttpClient\b|\bOkHttpClient\b|\bURLSession\b|\bNet::HTTP\b|^\s*(?:curl|wget)\s") },
    Io { key: "proc", label: "Commands to other programs", way: "out", pattern: Some(
        r"\bsubprocess\.|\bos\.system\(|\bexec\.Command\(|\bchild_process\b|\bexecSync\(|\bspawn\(|ProcessBuilder|Runtime\.getRuntime\(\)\.exec|std::process::Command|\bCommand::new\(|Process\.Start\(|(?:^|[^\w.>:])system\(|^\s*system\s|\bos\.exec\w*\(|\bsyscall\.Exec\(|^\s*exec\s|\bwebbrowser\.") },
    Io { key: "db", label: "Database", way: "both", pattern: Some(
        r#"\b(?:SELECT\s+[\w*,. ]+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+\s+SET|CREATE\s+TABLE)\b|\bsql\.Open\(|\.connect\(\s*['"](?:postgres|mysql|mongodb|redis|sqlite)"#) },
    Io { key: "queue", label: "Message queues", way: "both", pattern: None },   // known only from the libraries a file imports
];
pub static IO_RES: LazyLock<Vec<(&'static str, Regex)>> =
    LazyLock::new(|| IO.iter().filter_map(|io| io.pattern.map(|p| (io.key, rx(&format!("(?m){p}"))))).collect());
/// SQL, connection URLs and "${HOME:-...}" live inside strings
pub const IN_STRINGS: &[&str] = &["db", "env"];

/// library kind -> I/O
pub fn io_of_cat(cat: &str) -> Option<&'static str> {
    match cat {
        "db" => Some("db"),
        "queue" => Some("queue"),
        "http" | "cloud" => Some("http_out"),
        _ => None,
    }
}

pub fn io_order(key: &str) -> usize {
    IO.iter().position(|io| io.key == key).unwrap_or(usize::MAX)
}

/// 'github.com/gin-gonic/gin/render' -> web; the longest known prefix decides
pub fn cat_of(spec: &str) -> Option<&'static str> {
    let mut s = spec.to_lowercase();
    loop {
        if let Some(c) = EXT_CAT.get(s.as_str()) {
            return Some(c);
        }
        let cut = match (s.rfind('.'), s.rfind('/')) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) | (None, Some(a)) => a,
            _ => return None,
        };
        if cut == 0 {
            return None;
        }
        s.truncate(cut);
    }
}

/// 'parseHTTPRequest_v2' -> [parse, httprequest, v2]
pub fn toks(s: &str) -> Vec<String> {
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| rx(r"([a-z0-9])([A-Z])"));
    CAMEL
        .replace_all(s, "$1 $2")
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}
