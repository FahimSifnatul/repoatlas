// dev helper: cargo run --example dump -- <lang> <file>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lang: tree_sitter::Language = match a[1].as_str() {
        "py" => tree_sitter_python::LANGUAGE.into(),
        "js" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "kt" => tree_sitter_kotlin_ng::LANGUAGE.into(),
        "rb" => tree_sitter_ruby::LANGUAGE.into(),
        "php" => tree_sitter_php::LANGUAGE_PHP.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "cs" => tree_sitter_c_sharp::LANGUAGE.into(),
        "swift" => tree_sitter_swift::LANGUAGE.into(),
        "scala" => tree_sitter_scala::LANGUAGE.into(),
        "sh" => tree_sitter_bash::LANGUAGE.into(),
        _ => panic!(),
    };
    let src = std::fs::read_to_string(&a[2]).unwrap();
    let mut p = tree_sitter::Parser::new();
    p.set_language(&lang).unwrap();
    let t = p.parse(&src, None).unwrap();
    let mut c = t.walk();
    walk(&mut c, &src, 0);
}
fn walk(c: &mut tree_sitter::TreeCursor, src: &str, d: usize) {
    let n = c.node();
    if n.is_named() {
        let f = c.field_name().map(|f| format!("{f}: ")).unwrap_or_default();
        let txt = if n.child_count() == 0 { format!(" {:?}", &src[n.byte_range()]) } else { String::new() };
        println!("{}{}{}{}", "  ".repeat(d), f, n.kind(), txt);
    }
    if c.goto_first_child() {
        loop { walk(c, src, d + 1); if !c.goto_next_sibling() { break; } }
        c.goto_parent();
    }
}
