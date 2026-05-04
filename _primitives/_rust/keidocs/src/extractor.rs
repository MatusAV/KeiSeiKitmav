//! Language-aware documentation extractors.
//!
//! Each extractor returns a flat `Vec<Section>`; the markdown emitter is
//! responsible for grouping sections by `kind`.

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum SectionKind {
    Module,
    Item,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub target: Option<String>,
    pub body: String,
}

/// Parse rustdoc — module-level `//!` lines and item-level `///` blocks.
pub fn extract_rustdoc(content: &str) -> Vec<Section> {
    let mut out = Vec::new();
    let mut module_lines: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in content.lines() {
        if scan_rust_line(line, &mut module_lines, &mut current, &mut out) {
            continue;
        }
    }
    if !module_lines.is_empty() {
        out.insert(0, Section {
            kind: SectionKind::Module,
            target: None,
            body: module_lines.join("\n").trim().to_string(),
        });
    }
    out
}

fn scan_rust_line(
    line: &str,
    module_lines: &mut Vec<String>,
    current: &mut Vec<String>,
    out: &mut Vec<Section>,
) -> bool {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("//!") {
        module_lines.push(rest.trim_start().to_string());
        return true;
    }
    if let Some(rest) = t.strip_prefix("///") {
        current.push(rest.trim_start().to_string());
        return true;
    }
    if !current.is_empty() {
        out.push(Section {
            kind: SectionKind::Item,
            target: parse_rust_item_target(line),
            body: current.join("\n").trim().to_string(),
        });
        current.clear();
    }
    false
}

fn parse_rust_item_target(line: &str) -> Option<String> {
    let re = Regex::new(r"\b(pub\s+(?:fn|struct|enum|trait|const|type|mod)\s+\w+)").ok()?;
    re.captures(line).map(|c| c[1].to_string())
}

/// Parse jsdoc-style `/** ... */` blocks. Returns one Section per block.
pub fn extract_jsdoc(content: &str) -> Vec<Section> {
    let re = match Regex::new(r"(?s)/\*\*(.*?)\*/") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    re.captures_iter(content)
        .map(|c| {
            let raw = c[1].to_string();
            let body = raw
                .lines()
                .map(|l| l.trim_start().trim_start_matches('*').trim().to_string())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            Section {
                kind: SectionKind::Item,
                target: None,
                body,
            }
        })
        .collect()
}

/// Treat `# ` and `## ` markdown headers + leading paragraph as sections.
pub fn extract_md_headers(content: &str) -> Vec<Section> {
    let mut out = Vec::new();
    let mut header: Option<String> = None;
    let mut buf: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("# ") || line.starts_with("## ") {
            if let Some(h) = header.take() {
                out.push(Section {
                    kind: SectionKind::Item,
                    target: Some(h),
                    body: buf.join("\n").trim().to_string(),
                });
                buf.clear();
            }
            header = Some(line.trim_start_matches('#').trim().to_string());
        } else {
            buf.push(line.to_string());
        }
    }
    if let Some(h) = header {
        out.push(Section {
            kind: SectionKind::Item,
            target: Some(h),
            body: buf.join("\n").trim().to_string(),
        });
    }
    out
}
