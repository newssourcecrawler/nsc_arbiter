/// Loom/freeze flags from deterministic heuristics.
#[derive(Clone, Copy, Debug, Default)]
pub struct FreezeFlags {
    pub rep_3p: bool,   // high 3-gram repetition rate
    pub stall:  bool,   // very low diversity
    pub ai_tell: bool,  // "as an AI…" boilerplate
}

/// Allocation-free freeze flags, character-based.
#[inline]
pub fn freeze_flags(text: &str) -> FreezeFlags {
    let mut ff = FreezeFlags::default();
    let s = text.trim();
    if s.is_empty() { ff.stall = true; return ff; }

    // 3-gram repetition (character-based)
    let mut total = 0usize;
    let mut reps  = 0usize;
    if s.len() >= 6 {
        let b = s.as_bytes();
        let mut i = 0usize;
        while i + 6 <= b.len() {
            total += 1;
            if &b[i..i+3] == &b[i+3..i+6] { reps += 1; }
            i += 3;
        }
    }
    if total > 0 && (reps as f32) / (total as f32) > 0.30 { ff.rep_3p = true; }

    // Stall: unique-char diversity low
    let mut uniq = [false; 256];
    let mut seen = 0usize;
    for &ch in s.as_bytes() {
        let idx = ch as usize;
        if !uniq[idx] { uniq[idx] = true; seen += 1; }
    }
    if (seen as f32) / (s.len() as f32) < 0.12 { ff.stall = true; }

    let lo = s.to_ascii_lowercase();
    if lo.contains("as an ai") || lo.contains("as a language model") { ff.ai_tell = true; }
    ff
}