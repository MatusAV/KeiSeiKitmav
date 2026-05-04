//! 2-axis coverage metric: presence + agreement.
//!
//! - **presence** = `|blocks_with_formula| / |blocks_total|`.
//! - **agreement** = mean Jaccard over blocks with both declared and
//!   inferred effect-sets. For v0.1 PR-3 the inference pass is deferred
//!   to PR-4, so `agreement = 1.0` whenever no inferred sets are supplied
//!   (vacuous truth) — callers planning to gate CI on agreement should
//!   wait for PR-4 to feed real inferred sets in.
//!
//! Output is a `Coverage` struct serialisable to JSON by the CLI.

use kei_registry::EffectKind;
use serde::Serialize;
use std::collections::BTreeSet;

/// 2-axis coverage report for a single registry pass.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Coverage {
    pub blocks_total: usize,
    pub blocks_with_formula: usize,
    pub presence: f64,
    pub agreement: f64,
}

/// Compute coverage from raw counts and per-block effect-sets.
///
/// `pairs` carries one tuple per block-with-formula: `(declared, inferred)`.
/// Pass an empty `inferred` to opt out of the agreement axis (it
/// contributes Jaccard=1.0 to the mean — vacuous truth).
pub fn compute(
    blocks_total: usize,
    pairs: &[(BTreeSet<EffectKind>, BTreeSet<EffectKind>)],
) -> Coverage {
    let blocks_with_formula = pairs.len();
    let presence = if blocks_total == 0 {
        0.0
    } else {
        blocks_with_formula as f64 / blocks_total as f64
    };
    let agreement = if blocks_with_formula == 0 {
        1.0
    } else {
        let sum: f64 = pairs
            .iter()
            .map(|(d, i)| jaccard(d, i))
            .sum();
        sum / blocks_with_formula as f64
    };
    Coverage {
        blocks_total,
        blocks_with_formula,
        presence,
        agreement,
    }
}

/// Jaccard similarity for two effect sets. Empty-vs-empty = 1.0
/// (vacuous truth: nothing claimed and nothing inferred = perfect
/// agreement). Empty-vs-nonempty = 0.0 — the declarer either missed
/// effects or the inference is over-eager.
pub fn jaccard(a: &BTreeSet<EffectKind>, b: &BTreeSet<EffectKind>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}
