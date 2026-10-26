//! One entry per language: which files it owns, its grammar and query, and how its names are found.
//! Adding a language = a grammar crate, a file in queries/, and one entry in LANGS.

use std::collections::HashMap;
use std::sync::LazyLock;

use tree_sitter::{Language, Query};

/// Where a name is usable without an import.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// anywhere in the repo (C functions, Ruby, PHP, C# ...)
    Repo,
    /// in the same folder: one package per folder (Go, Java)
    Folder,
    /// only through an import, which the resolver follows (Python, JavaScript, Rust)
    Imports,
}

/// How a language's imports are turned into files of the repo (see resolve.rs).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Resolver {
    None,
    Python,
    Js,
    Go,
    Rust,
    Jvm,
    Php,
    C,
}

pub struct Lang {
    pub name: &'static str,
    pub exts: &'static [&'static str],
    /// a script with no extension starting "#!/usr/bin/env php" is this language
    pub interpreters: &'static [&'static str],
    pub color: &'static str,
    /// file names the scanner should remember: ("go.mod",)
    pub manifests: &'static [&'static str],
    /// files to ignore even though the extension matches: (".d.ts",)
    pub skip_suffixes: &'static [&'static str],
    /// grammar key (see GRAMMARS); None for pages and style sheets
    pub grammar: Option<&'static str>,
    /// look for app.get("/x")-style endpoints in the text
    pub regex_routes: bool,
    /// use the comment at the top of a file as its description (else its docstring)
    pub header_comments: bool,
    /// main.x / index.x / app.x may be a program entry point
    pub entry_by_name: bool,
    /// patterns that all must match for the file to be a program entry point
    pub entry_re: &'static [&'static str],
    pub default_layer: Option<&'static str>,
    /// a page or style sheet, not a program: never the project's main language
    pub markup: bool,
    /// file / folder-name words with a special meaning in this language
    pub stem_layers: &'static [(&'static str, &'static str)],
    pub scope: Scope,
    pub resolver: Resolver,
    /// a bare call inside a class body may be a method of that class (Java, C#, Ruby ...)
    pub implicit_this: bool,
    /// only the <script> blocks are code (Vue, Svelte)
    pub script_blocks: bool,
    /// every file of a folder loads together: reaching one runs the top level of all (Go package variables and `init()`)
    pub package_loads: bool,
}

const BASE: Lang = Lang {
    name: "",
    exts: &[],
    interpreters: &[],
    color: "#7d8aa5",
    manifests: &[],
    skip_suffixes: &[],
    grammar: None,
    regex_routes: true,
    header_comments: true,
    entry_by_name: true,
    entry_re: &[],
    default_layer: None,
    markup: false,
    stem_layers: &[],
    scope: Scope::Repo,
    resolver: Resolver::None,
    implicit_this: false,
    script_blocks: false,
    package_loads: false,
};

pub static LANGS: &[Lang] = &[
    Lang { name: "Python", exts: &[".py"], interpreters: &["python", "python3", "python2"], color: "#3572A5",
           manifests: &["pyproject.toml", "__init__.py"], grammar: Some("python"), regex_routes: false, header_comments: false,
           entry_re: &[r#"(?m)^if\s+__name__\s*==\s*['"]__main__['"]"#], stem_layers: &[("view", "api"), ("views", "api")],
           scope: Scope::Imports, resolver: Resolver::Python, ..BASE },
    Lang { name: "JavaScript", exts: &[".js", ".jsx", ".mjs", ".cjs"], interpreters: &["node", "nodejs", "bun", "deno"],
           color: "#d9b611", manifests: &["package.json"], grammar: Some("javascript"), scope: Scope::Imports,
           resolver: Resolver::Js, ..BASE },
    Lang { name: "TypeScript", exts: &[".ts", ".tsx"], color: "#3178c6", skip_suffixes: &[".d.ts"], grammar: Some("typescript"),
           scope: Scope::Imports, resolver: Resolver::Js, ..BASE },
    Lang { name: "Vue", exts: &[".vue"], color: "#41b883", grammar: Some("javascript"), entry_by_name: false,
           default_layer: Some("interface"), scope: Scope::Imports, resolver: Resolver::Js, script_blocks: true, ..BASE },
    Lang { name: "Svelte", exts: &[".svelte"], color: "#ff3e00", grammar: Some("javascript"), entry_by_name: false,
           default_layer: Some("interface"), scope: Scope::Imports, resolver: Resolver::Js, script_blocks: true, ..BASE },
    Lang { name: "Go", exts: &[".go"], color: "#00ADD8", manifests: &["go.mod"], grammar: Some("go"),
           entry_re: &[r"(?m)^package\s+main\b", r"(?m)^func\s+main\s*\("], scope: Scope::Folder, resolver: Resolver::Go, package_loads: true, ..BASE },
    Lang { name: "Rust", exts: &[".rs"], color: "#b7603c", manifests: &["Cargo.toml"], grammar: Some("rust"),
           entry_re: &[r"(?m)^\s*fn\s+main\s*\("], scope: Scope::Imports, resolver: Resolver::Rust, ..BASE },
    Lang { name: "Java", exts: &[".java"], color: "#b07219", grammar: Some("java"),
           entry_re: &[r"static\s+void\s+main\s*\(|@SpringBootApplication"], scope: Scope::Folder, resolver: Resolver::Jvm,
           implicit_this: true, ..BASE },
    Lang { name: "Kotlin", exts: &[".kt"], color: "#A97BFF", grammar: Some("kotlin"),
           entry_re: &[r"(?m)^fun\s+main\s*\(|@SpringBootApplication"], scope: Scope::Folder, resolver: Resolver::Jvm,
           implicit_this: true, ..BASE },
    Lang { name: "C", exts: &[".c", ".h"], color: "#555555", grammar: Some("c"), entry_re: &[r"\bint\s+main\s*\("],
           resolver: Resolver::C, ..BASE },
    Lang { name: "C++", exts: &[".cpp", ".cc", ".hpp"], color: "#f34b7d", grammar: Some("cpp"), entry_re: &[r"\bint\s+main\s*\("],
           resolver: Resolver::C, implicit_this: true, ..BASE },
    Lang { name: "C#", exts: &[".cs"], color: "#178600", grammar: Some("csharp"),
           entry_re: &[r"\bstatic\s+(?:async\s+)?[\w<>]+\s+Main\s*\("], implicit_this: true, ..BASE },
    // .ru: the Rack file a Ruby web server boots
    Lang { name: "Ruby", exts: &[".rb", ".ru", ".rake"], interpreters: &["ruby"], color: "#701516", grammar: Some("ruby"),
           entry_re: &[r"(?m)^if\s+(?:__FILE__\s*==\s*\$(?:0|PROGRAM_NAME)|\$(?:0|PROGRAM_NAME)\s*==\s*__FILE__)"],
           implicit_this: true, ..BASE },
    Lang { name: "PHP", exts: &[".php"], interpreters: &["php"], color: "#4F5D95", manifests: &["composer.json"], grammar: Some("php"),
           resolver: Resolver::Php, ..BASE },
    Lang { name: "Swift", exts: &[".swift"], color: "#F05138", grammar: Some("swift"), entry_re: &[r"(?m)^\s*@main\b"],
           implicit_this: true, ..BASE },
    Lang { name: "Scala", exts: &[".scala"], color: "#c22d40", grammar: Some("scala"),
           entry_re: &[r"\bdef\s+main\s*\(|\bextends\s+App\b"], implicit_this: true, ..BASE },
    // other scripts are named by path, which the core follows
    Lang { name: "Shell", exts: &[".sh", ".bash", ".zsh"], interpreters: &["sh", "bash", "zsh", "dash", "ash"], color: "#89e051",
           grammar: Some("bash"), scope: Scope::Imports, ..BASE },
    // pages link files by path (<script src>, href)
    Lang { name: "HTML", exts: &[".html"], color: "#e34c26", default_layer: Some("interface"), entry_by_name: false, markup: true,
           scope: Scope::Imports, ..BASE },
    // style sheets link files by path (@import, url())
    Lang { name: "CSS", exts: &[".css", ".scss"], color: "#563d7c", default_layer: Some("interface"), entry_by_name: false,
           markup: true, scope: Scope::Imports, ..BASE },
];

pub fn by_name(name: &str) -> &'static Lang {
    LANGS.iter().find(|l| l.name == name).expect("known language")
}

pub fn by_ext(ext: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.exts.contains(&ext))
}

pub fn by_interpreter(exe: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.interpreters.contains(&exe))
}

pub fn is_manifest(file_name: &str) -> bool {
    LANGS.iter().any(|l| l.manifests.contains(&file_name))
}

pub fn interpreters() -> impl Iterator<Item = &'static str> {
    LANGS.iter().flat_map(|l| l.interpreters.iter().copied())
}

pub struct Grammar {
    pub language: Language,
    pub query: Query,
}

/// grammar key -> parser and query, compiled once
pub static GRAMMARS: LazyLock<HashMap<&'static str, Grammar>> = LazyLock::new(|| {
    let js = include_str!("queries/javascript.scm");
    let jsx = include_str!("queries/jsx.scm");
    let ts = [js, include_str!("queries/typescript.scm")].concat();
    let table: Vec<(&str, Language, String)> = vec![
        ("python", tree_sitter_python::LANGUAGE.into(), include_str!("queries/python.scm").into()),
        ("javascript", tree_sitter_javascript::LANGUAGE.into(), [js, jsx].concat()),
        ("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), ts.clone()),
        ("tsx", tree_sitter_typescript::LANGUAGE_TSX.into(), [ts.as_str(), jsx].concat()),
        ("go", tree_sitter_go::LANGUAGE.into(), include_str!("queries/go.scm").into()),
        ("rust", tree_sitter_rust::LANGUAGE.into(), include_str!("queries/rust.scm").into()),
        ("java", tree_sitter_java::LANGUAGE.into(), include_str!("queries/java.scm").into()),
        ("kotlin", tree_sitter_kotlin_ng::LANGUAGE.into(), include_str!("queries/kotlin.scm").into()),
        ("c", tree_sitter_c::LANGUAGE.into(), include_str!("queries/c.scm").into()),
        ("cpp", tree_sitter_cpp::LANGUAGE.into(), include_str!("queries/cpp.scm").into()),
        ("csharp", tree_sitter_c_sharp::LANGUAGE.into(), include_str!("queries/csharp.scm").into()),
        ("ruby", tree_sitter_ruby::LANGUAGE.into(), include_str!("queries/ruby.scm").into()),
        ("php", tree_sitter_php::LANGUAGE_PHP.into(), include_str!("queries/php.scm").into()),
        ("swift", tree_sitter_swift::LANGUAGE.into(), include_str!("queries/swift.scm").into()),
        ("scala", tree_sitter_scala::LANGUAGE.into(), include_str!("queries/scala.scm").into()),
        ("bash", tree_sitter_bash::LANGUAGE.into(), include_str!("queries/bash.scm").into()),
    ];
    table
        .into_iter()
        .map(|(key, language, src)| {
            let query = Query::new(&language, &src).unwrap_or_else(|e| panic!("queries/{key}.scm: {e}"));
            (key, Grammar { language, query })
        })
        .collect()
});

/// The grammar for one file: .tsx needs the TSX dialect, a Vue <script lang="ts"> block TypeScript.
pub fn grammar_key(lang: &Lang, path: &str, ts_script: bool) -> Option<&'static str> {
    match lang.grammar? {
        "typescript" if path.ends_with(".tsx") => Some("tsx"),
        "javascript" if lang.script_blocks && ts_script => Some("typescript"),
        "javascript" if path.ends_with(".jsx") => Some("javascript"),
        g => Some(g),
    }
}
