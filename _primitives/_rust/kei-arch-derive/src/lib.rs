//! kei-arch-derive — bridge between kei-registry formulas and the
//! canonical `arch/PLAN.toml` evidence file.
//!
//! Phase 2 PR-3 of `arch/MATH-DNA-DESIGN.md`. Reads the registry SQLite,
//! walks the workspace for `[package.metadata.keisei.formula]` declarations
//! in member `Cargo.toml`s, and projects the formula 4-tuple onto the
//! seven `kei_arch_map::schema::Evidence` kinds already shipped in PR-1.
//!
//! Constructor Pattern: each module is one cube with one responsibility.
//! `project` owns the predicate→evidence mapping table; `walker` owns
//! Cargo.toml discovery; `emit` owns deterministic TOML serialisation;
//! `coverage` owns the 2-axis (presence + agreement) metric.

pub mod coverage;
pub mod emit;
pub mod infer;
pub mod project;
pub mod serialize;
pub mod walker;

pub use coverage::{compute as compute_coverage, Coverage};
pub use emit::{emit_plan, render_plan_string, DerivedClaim, DerivedModule, DerivedPlan};
pub use project::{predicate_to_evidence, EvidenceClaim};
pub use walker::{discover_formulas, walk_blocks, FormulaDecl};
