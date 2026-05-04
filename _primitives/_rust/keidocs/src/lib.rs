//! keidocs — auto-extract per-file documentation with DNA frontmatter.
//!
//! Public API:
//! - [`extractor`] — language-specific doc-comment parsers
//! - [`dna`] — content-addressable file fingerprint
//! - [`render`] — markdown frontmatter + section emitter

pub mod dna;
pub mod extractor;
pub mod render;
