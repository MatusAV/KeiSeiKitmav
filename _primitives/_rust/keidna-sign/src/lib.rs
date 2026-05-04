//! keidna-sign — produces signed DNA manifest for KeiSeiKit primitives.
//!
//! Phase 1: sha256 content-addressed manifest (deterministic).
//! Phase 2 (future): ed25519 signing layer over the dna_hash.

pub mod manifest;

pub use manifest::{
    compute_primitive_dna, dna_path, read_from, verify, write_to,
    DnaManifest, FileEntry, Lineage,
};
