//! Coverage 2-axis (presence + agreement) tests.
//!
//! Hand-built effect-sets exercise the Jaccard arithmetic plus the
//! vacuous-truth path (empty registry → presence 0, agreement 1).

use kei_arch_derive::compute_coverage;
use kei_arch_derive::coverage::jaccard;
use kei_registry::EffectKind;
use std::collections::BTreeSet;

fn set_of(kinds: &[EffectKind]) -> BTreeSet<EffectKind> {
    kinds.iter().cloned().collect()
}

#[test]
fn empty_registry_has_zero_presence_and_one_agreement() {
    let cov = compute_coverage(0, &[]);
    assert_eq!(cov.blocks_total, 0);
    assert_eq!(cov.blocks_with_formula, 0);
    assert_eq!(cov.presence, 0.0);
    assert_eq!(cov.agreement, 1.0);
}

#[test]
fn presence_is_blocks_with_formula_over_total() {
    let pairs = vec![
        (
            set_of(&[EffectKind::Stdout]),
            set_of(&[EffectKind::Stdout]),
        ),
        (
            set_of(&[EffectKind::Stderr]),
            set_of(&[EffectKind::Stderr]),
        ),
    ];
    let cov = compute_coverage(4, &pairs);
    assert_eq!(cov.blocks_total, 4);
    assert_eq!(cov.blocks_with_formula, 2);
    assert!((cov.presence - 0.5).abs() < 1e-9);
    assert!((cov.agreement - 1.0).abs() < 1e-9);
}

#[test]
fn agreement_is_mean_jaccard_over_pairs() {
    // Pair 1: Jaccard = 1/2 (intersection {Stdout}, union {Stdout, Stderr})
    // Pair 2: Jaccard = 1/1 = 1.0
    let pair1 = (
        set_of(&[EffectKind::Stdout]),
        set_of(&[EffectKind::Stdout, EffectKind::Stderr]),
    );
    let pair2 = (
        set_of(&[EffectKind::GitMutate]),
        set_of(&[EffectKind::GitMutate]),
    );
    let cov = compute_coverage(2, &[pair1, pair2]);
    let expected = (0.5 + 1.0) / 2.0;
    assert!((cov.agreement - expected).abs() < 1e-9);
}

#[test]
fn jaccard_empty_sets_is_one() {
    let a: BTreeSet<EffectKind> = BTreeSet::new();
    let b: BTreeSet<EffectKind> = BTreeSet::new();
    assert_eq!(jaccard(&a, &b), 1.0);
}

#[test]
fn jaccard_disjoint_sets_is_zero() {
    let a = set_of(&[EffectKind::Stdout]);
    let b = set_of(&[EffectKind::Stderr]);
    assert_eq!(jaccard(&a, &b), 0.0);
}

#[test]
fn jaccard_identical_sets_is_one() {
    let a = set_of(&[EffectKind::Stdout, EffectKind::Stderr]);
    let b = set_of(&[EffectKind::Stdout, EffectKind::Stderr]);
    assert_eq!(jaccard(&a, &b), 1.0);
}
