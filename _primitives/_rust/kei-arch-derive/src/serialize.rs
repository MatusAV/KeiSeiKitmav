//! Inline-evidence TOML rendering. Produces the single-line inline-table
//! shape used by hand-written `arch/PLAN.toml` so the auto-generated and
//! hand-edited files round-trip through `kei_arch_map::schema::load`.
//!
//! Constructor Pattern: this cube ONLY serialises one `EvidenceClaim` to
//! one TOML inline-table line. Bulk plan rendering lives in `emit.rs`.

use crate::project::EvidenceClaim;

/// Render an `EvidenceClaim` as a single TOML inline-table string.
pub fn inline_evidence(ev: &EvidenceClaim) -> String {
    match ev {
        EvidenceClaim::FileExists { path } => render_file_exists(path),
        EvidenceClaim::RegexMatch { file, pattern } => render_regex_match(file, pattern),
        EvidenceClaim::GrepCount {
            file,
            pattern,
            expected,
        } => render_grep_count(file, pattern, *expected),
        EvidenceClaim::FileSize { path, range } => render_file_size(path, range),
        EvidenceClaim::JsonField {
            file,
            path,
            expected,
        } => render_json_field(file, path, expected),
        EvidenceClaim::CargoCheckClean { manifest_dir } => render_cargo_check(manifest_dir),
        EvidenceClaim::HttpStatus { url, expected } => render_http_status(url, expected),
    }
}

fn render_file_exists(path: &std::path::Path) -> String {
    format!(
        "{{ kind = \"file_exists\", path = {} }}",
        quote(&path.display().to_string())
    )
}

fn render_regex_match(file: &std::path::Path, pattern: &str) -> String {
    format!(
        "{{ kind = \"regex_match\", file = {}, pattern = {} }}",
        quote(&file.display().to_string()),
        quote(pattern)
    )
}

fn render_grep_count(file: &std::path::Path, pattern: &str, expected: u64) -> String {
    format!(
        "{{ kind = \"grep_count\", file = {}, pattern = {}, expected = {} }}",
        quote(&file.display().to_string()),
        quote(pattern),
        expected
    )
}

fn render_file_size(path: &std::path::Path, range: &[u64; 2]) -> String {
    format!(
        "{{ kind = \"file_size\", path = {}, range = [{}, {}] }}",
        quote(&path.display().to_string()),
        range[0],
        range[1]
    )
}

fn render_json_field(file: &std::path::Path, path: &str, expected: &str) -> String {
    format!(
        "{{ kind = \"json_field\", file = {}, path = {}, expected = {} }}",
        quote(&file.display().to_string()),
        quote(path),
        quote(expected)
    )
}

fn render_cargo_check(manifest_dir: &std::path::Path) -> String {
    format!(
        "{{ kind = \"cargo_check_clean\", manifest_dir = {} }}",
        quote(&manifest_dir.display().to_string())
    )
}

fn render_http_status(url: &str, expected: &[u16]) -> String {
    let nums = expected
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{ kind = \"http_status\", url = {}, expected = [{}] }}",
        quote(url),
        nums
    )
}

/// TOML basic-string quoting: backslash and double-quote escape, wrap in `"`.
pub fn quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}
