//! repoatlas: turn any code repository into an interactive, layer-by-layer architecture map, with a
//! step-by-step journey through each program it runs.
#![allow(clippy::type_complexity)]

pub mod atlas;
pub mod finder;
pub mod graph;
pub mod journey;
pub mod lang;
pub mod launch;
pub mod layers;
pub mod mounts;
pub mod orm;
pub mod parse;
pub mod resolve;
pub mod rules;
pub mod scan;
pub mod scip;
pub mod stdlib;
pub mod tables;
pub mod text;

pub use atlas::{analyze, analyze_with};

/// The map as one self-contained HTML page.
pub fn render(data: &serde_json::Value, title: &str) -> String {
    let tpl = include_str!("viewer.html");
    // every "<" escaped: "</script>" or "<!--<script" in a file name can then never end or derail the <script> block
    let blob = serde_json::to_string(data).unwrap_or_else(|_| "null".into()).replace('<', "\\u003c");
    let title = title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;");
    tpl.replace("__TITLE__", &title).replace("/*__DATA__*/null", &blob)
}
