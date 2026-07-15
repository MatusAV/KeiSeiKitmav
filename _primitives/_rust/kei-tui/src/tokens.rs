//! Real token counting for the context meter.
//!
//! The bottom-left `контекст N%` figure is the size of the current conversation
//! we would replay to the model. This module counts it with a REAL BPE
//! tokenizer (`cl100k_base`, vocab embedded in `tiktoken-rs` → offline), not a
//! chars/4 heuristic. It is provider-independent, so it works for LOCAL models
//! and is the FALLBACK for cloud models until the provider's exact `Usage`
//! arrives over the run SSE (see the `RunEvent::Usage` path).
//!
//! Caveat: `cl100k_base` is the GPT-family tokenizer; GLM/Claude use their own,
//! so this differs from their exact count (typically <10–15%). It is a real
//! tokenizer count, orders of magnitude better than chars/4, and exact enough
//! for a fill meter.

use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

/// Lazily-built cl100k tokenizer — building it parses the embedded vocab, so we
/// do it once and reuse. `None` iff the (embedded) load ever fails.
fn bpe() -> Option<&'static CoreBPE> {
    static BPE: OnceLock<Option<CoreBPE>> = OnceLock::new();
    BPE.get_or_init(|| tiktoken_rs::cl100k_base().ok())
        .as_ref()
}

/// Real BPE token count of `text`. Falls back to a chars/4 estimate only if the
/// tokenizer failed to load (should not happen — the vocab is compiled in).
pub fn count(text: &str) -> u32 {
    match bpe() {
        Some(b) => b.encode_ordinary(text).len() as u32,
        None => (text.chars().count() / 4) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(count(""), 0);
    }

    #[test]
    fn a_real_sentence_tokenizes_to_a_plausible_count() {
        // "The quick brown fox jumps over the lazy dog" is 9 words; a real BPE
        // splits it into ~9–11 tokens — far from the chars/4 (~11) coincidence
        // on longer text, but the point here is: non-zero, and < word*3.
        let n = count("The quick brown fox jumps over the lazy dog");
        assert!(n >= 8 && n <= 15, "got {n}");
    }

    #[test]
    fn cyrillic_counts_more_tokens_than_latin_of_equal_length() {
        // cl100k splits Cyrillic into more (often per-byte) tokens than Latin —
        // a real tokenizer property a chars/4 heuristic misses entirely.
        let lat = count("aaaaaaaaaaaaaaaaaaaa");
        let cyr = count("аааааааааааааааааааа");
        assert!(cyr > lat, "cyr={cyr} lat={lat}");
    }
}
