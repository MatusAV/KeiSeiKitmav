//! The "thinking" indicator: a spinning contrast ball (◐◓◑◒ — our Frobenius
//! sphere) followed by a phrase that DECODES out of the matrix. Each character
//! starts as a scrambling matrix glyph and settles, left-to-right, into a real
//! English word — so the reveal MEANS something (it's not dumb noise). No
//! formulas trailing it.

/// Pulsing-orb frames — the "thinking" sphere. ONLY █▓▒░ (the block glyphs
/// Ubuntu Mono renders; ◐◓◑◒ █ are TOFU in it), so it never shows as a box.
/// Reads as a breathing/glowing ball.
const FRAMES: [&str; 8] = ["░", "▒", "▓", "█", "█", "▓", "▒", "░"];

/// Matrix glyphs the unrevealed characters scramble through — RESTRICTED to the
/// set Ubuntu Sans Mono actually renders (checked against its cmap): ascii
/// symbols, the covered math operators, some Greek, box-drawing + shading. No
/// arrows/geometric/exotic — those are tofu boxes in this font.
const MATRIX: &[char] = &[
    // digits + ascii symbols (all render)
    '0', '1', '2', '3', '7', '9', '#', '@', '&', '%', '$', '<', '>', '{', '}', '/', '=', '+', '*',
    // math operators Ubuntu Mono covers
    '∑', '∏', '∫', '≈', '≠', '±', '√', '∞',
    // Greek Ubuntu Mono covers
    'α', 'β', 'γ', 'λ', 'φ',
    // box-drawing + shading (full coverage) — the "code" texture
    '░', '▒', '▓', '█', '─', '│', '┌', '┐', '└', '┘', '═', '║', '╔', '╗', '╚', '╝', '┤', '├',
];

/// English phrases the decoder resolves into — meaningful, our resonance
/// flavour with a little humour. Each shows for ~2.8 s then the next decodes.
const PHRASES: &[&str] = &[
    "deriving theorems",
    "running experiments",
    "resonating on the unit sphere",
    "seeking the fixed point",
    "normalizing by frobenius",
    "composing kubiks",
    "refuting the falsifier",
    "crystallizing the solution",
    "bootstrapping to convergence",
    "reading the project passport",
    "thinking in state matrices",
    "distilling signal from noise",
];

const PHRASE_MS: u128 = 2800; // how long each phrase is shown
const LOCK_MS: u128 = 90; // per-character left-to-right reveal step
const SCRAMBLE_MS: u128 = 60; // matrix-glyph flip rate

/// The current contrast-ball glyph for an elapsed time in ms (~7 fps spin).
pub fn glyph(elapsed_ms: u128) -> &'static str {
    FRAMES[((elapsed_ms / 140) % FRAMES.len() as u128) as usize]
}

/// The decoded phrase for `elapsed_ms`: revealed chars are real, the rest
/// scramble through matrix glyphs. Deterministic (indices derived from time,
/// no RNG).
pub fn decode(elapsed_ms: u128) -> String {
    let phrase = PHRASES[((elapsed_ms / PHRASE_MS) % PHRASES.len() as u128) as usize];
    let t = elapsed_ms % PHRASE_MS; // time within this phrase
    let frame = elapsed_ms / SCRAMBLE_MS;
    let mut out = String::new();
    for (i, ch) in phrase.chars().enumerate() {
        if ch == ' ' {
            out.push(' ');
            continue;
        }
        let reveal_at = 200 + (i as u128) * LOCK_MS;
        if t >= reveal_at {
            out.push(ch);
        } else {
            let idx = ((i as u128) * 13 + frame * 7) % MATRIX.len() as u128;
            out.push(MATRIX[idx as usize]);
        }
    }
    out
}

/// One-line thinking indicator: spinning ball + the decoding phrase.
pub fn line(elapsed_ms: u128) -> String {
    format!("{}  {}", glyph(elapsed_ms), decode(elapsed_ms))
}
