//! The 32 algorithms: which operator modulates which, which ones are heard, and where the feedback
//! loop is. Written from the diagrams printed on the instrument's panel and in its manual. Operators
//! are numbered as there, 1 to 6, and an operator is only ever modulated by higher-numbered ones
//! (the feedback aside), so playing 6 down to 1 has each one's modulators ready.

/// One algorithm. All indexed by operator number less one.
#[derive(Debug, Clone, Copy)]
pub struct Algorithm {
    /// Each operator's modulators (operator indices), as many as `mod_count` says.
    pub mods: [[u8; 3]; 6],
    pub mod_count: [u8; 6],
    /// Bit `n` set: operator `n + 1` goes to the output.
    pub carriers: u8,
    /// Whose last output goes through the feedback, and which operator it goes into. Most feed
    /// themselves; two algorithms feed one operator from another, a longer loop.
    pub fb_from: u8,
    pub fb_to: u8,
}

/// One algorithm in the printed way: the modulations `m > c`, the carriers, and the loop.
const fn algo(edges: &[(u8, u8)], carriers: &[u8], fb: (u8, u8)) -> Algorithm {
    let mut mods = [[0u8; 3]; 6];
    let mut mod_count = [0u8; 6];
    let mut k = 0;
    while k < edges.len() {
        let (m, c) = edges[k];
        let c0 = (c - 1) as usize;
        mods[c0][mod_count[c0] as usize] = m - 1;
        mod_count[c0] += 1;
        k += 1;
    }
    let mut bits = 0u8;
    let mut k = 0;
    while k < carriers.len() {
        bits |= 1 << (carriers[k] - 1);
        k += 1;
    }
    Algorithm { mods, mod_count, carriers: bits, fb_from: fb.0 - 1, fb_to: fb.1 - 1 }
}

pub const ALGORITHMS: [Algorithm; 32] = [
    /* 1 */ algo(&[(2, 1), (6, 5), (5, 4), (4, 3)], &[1, 3], (6, 6)),
    /* 2 */ algo(&[(2, 1), (6, 5), (5, 4), (4, 3)], &[1, 3], (2, 2)),
    /* 3 */ algo(&[(3, 2), (2, 1), (6, 5), (5, 4)], &[1, 4], (6, 6)),
    /* 4 */ algo(&[(3, 2), (2, 1), (6, 5), (5, 4)], &[1, 4], (4, 6)),
    /* 5 */ algo(&[(2, 1), (4, 3), (6, 5)], &[1, 3, 5], (6, 6)),
    /* 6 */ algo(&[(2, 1), (4, 3), (6, 5)], &[1, 3, 5], (5, 6)),
    /* 7 */ algo(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], (6, 6)),
    /* 8 */ algo(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], (4, 4)),
    /* 9 */ algo(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], (2, 2)),
    /* 10 */ algo(&[(3, 2), (2, 1), (5, 4), (6, 4)], &[1, 4], (3, 3)),
    /* 11 */ algo(&[(3, 2), (2, 1), (5, 4), (6, 4)], &[1, 4], (6, 6)),
    /* 12 */ algo(&[(2, 1), (4, 3), (5, 3), (6, 3)], &[1, 3], (2, 2)),
    /* 13 */ algo(&[(2, 1), (4, 3), (5, 3), (6, 3)], &[1, 3], (6, 6)),
    /* 14 */ algo(&[(2, 1), (4, 3), (5, 4), (6, 4)], &[1, 3], (6, 6)),
    /* 15 */ algo(&[(2, 1), (4, 3), (5, 4), (6, 4)], &[1, 3], (2, 2)),
    /* 16 */ algo(&[(2, 1), (3, 1), (4, 3), (5, 1), (6, 5)], &[1], (6, 6)),
    /* 17 */ algo(&[(2, 1), (3, 1), (4, 3), (5, 1), (6, 5)], &[1], (2, 2)),
    /* 18 */ algo(&[(2, 1), (3, 1), (4, 1), (5, 4), (6, 5)], &[1], (3, 3)),
    /* 19 */ algo(&[(3, 2), (2, 1), (6, 4), (6, 5)], &[1, 4, 5], (6, 6)),
    /* 20 */ algo(&[(3, 1), (3, 2), (5, 4), (6, 4)], &[1, 2, 4], (3, 3)),
    /* 21 */ algo(&[(3, 1), (3, 2), (6, 4), (6, 5)], &[1, 2, 4, 5], (3, 3)),
    /* 22 */ algo(&[(2, 1), (6, 3), (6, 4), (6, 5)], &[1, 3, 4, 5], (6, 6)),
    /* 23 */ algo(&[(3, 2), (6, 4), (6, 5)], &[1, 2, 4, 5], (6, 6)),
    /* 24 */ algo(&[(6, 3), (6, 4), (6, 5)], &[1, 2, 3, 4, 5], (6, 6)),
    /* 25 */ algo(&[(6, 4), (6, 5)], &[1, 2, 3, 4, 5], (6, 6)),
    /* 26 */ algo(&[(3, 2), (5, 4), (6, 4)], &[1, 2, 4], (6, 6)),
    /* 27 */ algo(&[(3, 2), (5, 4), (6, 4)], &[1, 2, 4], (3, 3)),
    /* 28 */ algo(&[(2, 1), (4, 3), (5, 4)], &[1, 3, 6], (5, 5)),
    /* 29 */ algo(&[(4, 3), (6, 5)], &[1, 2, 3, 5], (6, 6)),
    /* 30 */ algo(&[(4, 3), (5, 4)], &[1, 2, 3, 6], (5, 5)),
    /* 31 */ algo(&[(6, 5)], &[1, 2, 3, 4, 5], (6, 6)),
    /* 32 */ algo(&[], &[1, 2, 3, 4, 5, 6], (6, 6)),
];
