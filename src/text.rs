//! Text helpers: one-line descriptions from comments, docstrings and READMEs.

use std::sync::LazyLock;

use regex::Regex;

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

pub fn squash_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most n characters, cut at a sentence end when one is near, else at a word with "…".
pub fn trim(s: &str, n: usize) -> String {
    let s = squash_ws(s);
    if s.chars().count() <= n {
        return s;
    }
    let cut: String = s.chars().take(n).collect();
    let m = [". ", "! ", "? "].iter().filter_map(|p| cut.rfind(p)).max();
    match m {
        Some(m) if cut[..m].chars().count() as f64 > n as f64 * 0.5 => cut[..m + 1].to_string(),
        _ => match cut.rfind(' ') {
            Some(sp) => format!("{}…", &cut[..sp]),
            None => format!("{cut}…"),
        },
    }
}

pub fn first_sentence(s: &str, n: usize) -> String {
    static SENT: LazyLock<Regex> = LazyLock::new(|| rx(r"^(.+?[.!?])(\s|$)"));
    let s = squash_ws(s);
    match SENT.captures(&s) {
        Some(c) => trim(&c[1], n),
        None => trim(&s, n),
    }
}

pub fn line_of(text: &str, pos: usize) -> usize {
    text.as_bytes()[..pos.min(text.len())].iter().filter(|&&b| b == b'\n').count() + 1
}

/// Comment block directly above line i (0-based), for any C-like / hash-comment language.
pub fn lead_comment(lines: &[&str], i: usize) -> String {
    static C_LIKE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(//+|/\*+|\*+/?|--)"));
    static NOT_HASH: LazyLock<Regex> = LazyLock::new(|| rx(r"^#\s*(include|define|if|ifdef|ifndef|endif|else|pragma|import|undef)|^#[\[!]"));
    static MARK: LazyLock<Regex> = LazyLock::new(|| rx(r"^(//+!?|#+|/\*+|\*+/?|--)\s?|\*/$"));
    let mut out = Vec::new();
    let mut j = i;
    while j > 0 && out.len() < 6 {
        j -= 1;
        let s = lines[j].trim();
        let is_c = C_LIKE.is_match(s) || s.ends_with("*/");
        let is_h = s.starts_with('#') && !NOT_HASH.is_match(s);
        if !(is_c || is_h) {
            break;
        }
        out.push(MARK.replace_all(s, "").trim().to_string());
    }
    let text: Vec<String> = out.into_iter().rev().filter(|t| !t.is_empty() && !t.starts_with('@')).collect();
    first_sentence(&text.join(" "), 150)
}

/// First sentence of the comment block at the very top of a file (skips licences and linter directives).
pub fn header_doc(text: &str) -> String {
    static SHEBANG: LazyLock<Regex> = LazyLock::new(|| rx(r"^\x{feff}?(#!.*\n)?"));
    static STRICT: LazyLock<Regex> = LazyLock::new(|| rx(r#"^\s*(?:'use strict'|"use strict");?\s*"#));
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| rx(r"(?s)^\s*/\*+(.*?)\*/"));
    static NOT_HASH: LazyLock<Regex> = LazyLock::new(|| rx(r"^#\s*(include|define|if|pragma|import)|^#[\[!]"));
    static MARK: LazyLock<Regex> = LazyLock::new(|| rx(r"^(//+!?|#+)\s?"));
    static LICENSE: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?i)copyright|licen[sc]e|permission is hereby|spdx|all rights reserved|warranty"));
    static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
        rx(r"(?i)^(eslint|tslint|prettier|jshint|istanbul|@ts-|@flow|global |noqa|pylint|type:|use strict|vim:|-\*-)")
    });
    static STAR: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*\*+\s?"));
    let t = SHEBANG.replace(text, "");
    let t = STRICT.replace(&t, "");
    let body = if let Some(c) = BLOCK.captures(&t) {
        c[1].to_string()
    } else {
        let mut block = Vec::new();
        for ln in t.trim_start().lines() {
            let s = ln.trim();
            if s.starts_with("//") || (s.starts_with('#') && !NOT_HASH.is_match(s)) {
                block.push(MARK.replace(s, "").to_string());
            } else {
                break;
            }
        }
        block.join("\n")
    };
    if body.is_empty() || LICENSE.is_match(&body) || DIRECTIVE.is_match(body.trim().trim_start_matches(['*', '!', ' ', '\n'])) {
        return String::new();
    }
    let lines: Vec<String> = body
        .lines()
        .map(|l| STAR.replace(l, "").trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('@'))
        .collect();
    first_sentence(&lines.join(" "), 150)
}

/// The first real paragraph of a README (no headings, badges, code or tables).
pub fn read_para(txt: &str) -> String {
    static PARA: LazyLock<Regex> = LazyLock::new(|| rx(r"\n\s*\n"));
    static LINK: LazyLock<Regex> = LazyLock::new(|| rx(r"!?\[([^\]]*)\]\([^)]*\)"));
    static TAG: LazyLock<Regex> = LazyLock::new(|| rx(r"<[^>]+>|[`*]"));
    for para in PARA.split(txt) {
        let lines: Vec<&str> = para.trim().lines().map(str::trim).collect();
        if lines.is_empty() || lines[0].is_empty() {
            continue;
        }
        if ["#", "=", "-", "!", "[![", "<", "|", "```", ">", ":", ".."].iter().any(|p| lines[0].starts_with(p)) {
            continue;
        }
        if lines.iter().any(|l| l.chars().count() > 2 && l.chars().all(|c| "=-~#".contains(c))) {
            continue;
        }
        let s = lines.join(" ");
        let s = LINK.replace_all(&s, "$1");
        let s = TAG.replace_all(&s, "");
        if s.chars().count() > 25 {
            return trim(&s, 260);
        }
    }
    String::new()
}

/// 12345 -> "12,345"
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// s[a..b], moved inwards to the nearest character boundaries.
pub fn slice(s: &str, a: usize, b: usize) -> &str {
    let (mut a, mut b) = (a.min(s.len()), b.min(s.len()));
    while a < b && !s.is_char_boundary(a) {
        a += 1;
    }
    while b > a && !s.is_char_boundary(b) {
        b -= 1;
    }
    &s[a..b]
}
