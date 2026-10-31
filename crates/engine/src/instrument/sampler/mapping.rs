//! Placing files on keys: telling the note, velocity layer and round-robin turn from a file's name,
//! and splitting the keyboard between them.
//!
//! Note names read Logic's way, as the rest of Fermata shows them: middle C (MIDI 60) is C3.

use super::state::Zone;

/// What a file's name says about it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NameInfo {
    pub note: Option<u8>,
    /// A layer's loudness, for ordering layers: `v3`, `vel90`, or a dynamic like `mf`.
    pub layer: Option<u32>,
    /// A round-robin turn: `rr2`.
    pub turn: Option<u32>,
}

/// A note name like `C3`, `F#2`, `Bb-1`, `eb4`.
fn parse_note(t: &str) -> Option<u8> {
    let mut c = t.chars();
    let base = match c.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = c.as_str();
    let (acc, rest) = match rest.chars().next()? {
        '#' => (1, &rest[1..]),
        'b' if rest.len() > 1 => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i32 = rest.parse().ok()?;
    if !(-2..=8).contains(&octave) {
        return None;
    }
    let n = (octave + 2) * 12 + base + acc;
    (0..=127).contains(&n).then_some(n as u8)
}

const DYNAMICS: [&str; 8] = ["ppp", "pp", "p", "mp", "mf", "f", "ff", "fff"];

/// Reads the name: words are split at anything but letters, digits, `#` and a `-` before a digit.
pub fn parse_name(name: &str) -> NameInfo {
    let mut out = NameInfo::default();
    // Split at separators; keep "-1" with the note before it.
    let words: Vec<&str> = name.split(|c: char| !(c.is_alphanumeric() || c == '#' || c == '-')).filter(|w| !w.is_empty()).collect();
    let tokens: Vec<&str> = words.iter().flat_map(|w| split_dashes(w)).collect();
    let mut plain_number = None;
    for t in tokens.iter().rev() {
        let low = t.to_ascii_lowercase();
        if out.turn.is_none() {
            if let Some(n) = low.strip_prefix("rr").or(low.strip_prefix("seq")).and_then(|d| d.parse().ok()) {
                out.turn = Some(n);
                continue;
            }
        }
        if out.layer.is_none() {
            if let Some(n) = low.strip_prefix("vel").or(low.strip_prefix("vl")).or(low.strip_prefix('v')).and_then(|d| d.parse().ok()) {
                out.layer = Some(n);
                continue;
            }
            if let Some(k) = DYNAMICS.iter().position(|d| *d == low) {
                out.layer = Some(k as u32);
                continue;
            }
        }
        if out.note.is_none() {
            if let Some(n) = parse_note(t) {
                out.note = Some(n);
                continue;
            }
        }
        if plain_number.is_none() && t.len() <= 3 {
            plain_number = t.parse::<u8>().ok().filter(|n| (21..=108).contains(n));
        }
    }
    // A bare number only counts as a note, in piano range, when no name was found.
    out.note = out.note.or(plain_number);
    out
}

fn split_dashes(w: &str) -> Vec<&str> {
    // "Piano-C-1" is the note C-1; "Bass-C3" is two words. A dash joins only when it reads as an
    // octave below zero.
    let mut out = Vec::new();
    let mut from = 0;
    let b = w.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'-' {
            let negative_octave = i > 0 && i + 1 < b.len() && b[i + 1].is_ascii_digit() && parse_note(&w[from..i + 2]).is_some() && (i + 2 == b.len() || !b[i + 2].is_ascii_alphanumeric());
            if !negative_octave {
                if i > from {
                    out.push(&w[from..i]);
                }
                from = i + 1;
            }
        }
    }
    if from < b.len() {
        out.push(&w[from..]);
    }
    out
}

/// A file about to be mapped.
pub struct NewFile {
    pub zone: Zone,
    /// The root from the file's own `smpl` chunk, which outranks its name.
    pub info_root: Option<u8>,
    pub name: NameInfo,
}

/// Maps files to keys:
///
/// - One file: over the whole keyboard (or just `at_key`).
/// - Several whose notes can all be told: each around its note, with the keyboard split halfway
///   between neighbours. Files on the same note become velocity layers (in order of the layer in
///   their names, then quiet to loud as named), and round-robin turns in the same place share a
///   group.
/// - Otherwise chromatically, one key each, from `at_key` (C3 unless given), each at its own pitch.
pub fn map_files(files: Vec<NewFile>, at_key: Option<u8>, next_group: &mut u16) -> Vec<Zone> {
    if files.is_empty() {
        return Vec::new();
    }
    let root_of = |f: &NewFile| f.info_root.or(f.name.note);
    if files.len() == 1 {
        let f = files.into_iter().next().unwrap();
        let root = f.info_root.or(f.name.note).unwrap_or(60);
        let mut z = f.zone;
        z.root = root;
        if let Some(k) = at_key {
            z.key_lo = k;
            z.key_hi = k;
            z.root = k;
        }
        return vec![z];
    }
    if !files.iter().all(|f| root_of(f).is_some()) {
        let from = at_key.unwrap_or(60) as usize;
        let mut out = Vec::new();
        for (i, f) in files.into_iter().enumerate() {
            let mut z = f.zone;
            let k = (from + i).min(127) as u8;
            z.key_lo = k;
            z.key_hi = k;
            z.root = k;
            out.push(z);
        }
        return out;
    }
    // Group by note, then split the keyboard between the notes found.
    let mut notes: Vec<u8> = files.iter().filter_map(root_of).collect();
    notes.sort_unstable();
    notes.dedup();
    let range = |n: u8| {
        let at = notes.iter().position(|x| *x == n).unwrap();
        let lo = if at == 0 { 0 } else { ((notes[at - 1] as u16 + n as u16) / 2 + 1) as u8 };
        let hi = if at + 1 == notes.len() { 127 } else { ((n as u16 + notes[at + 1] as u16) / 2) as u8 };
        (lo, hi)
    };
    let mut out = Vec::new();
    for &n in &notes {
        let mut here: Vec<NewFile> = Vec::new();
        for f in &files {
            if root_of(f) == Some(n) {
                here.push(NewFile { zone: f.zone.clone(), info_root: f.info_root, name: f.name });
            }
        }
        here.sort_by_key(|f| (f.name.layer.unwrap_or(0), f.name.turn.unwrap_or(0)));
        // Layers, each with its turns.
        let mut layers: Vec<u32> = here.iter().map(|f| f.name.layer.unwrap_or(0)).collect();
        layers.dedup();
        for (i, layer) in layers.iter().enumerate() {
            let vel_lo = (i * 128 / layers.len()) as u8;
            let vel_hi = (((i + 1) * 128 / layers.len()) - 1) as u8;
            let of_layer: Vec<&NewFile> = here.iter().filter(|f| f.name.layer.unwrap_or(0) == *layer).collect();
            let group = if of_layer.len() > 1 {
                *next_group += 1;
                (*next_group).min(super::map::MAX_RR_GROUPS as u16 - 1)
            } else {
                0
            };
            for f in of_layer {
                let mut z = f.zone.clone();
                let (lo, hi) = range(n);
                z.key_lo = lo;
                z.key_hi = hi;
                z.vel_lo = vel_lo;
                z.vel_hi = vel_hi;
                z.root = n;
                z.rr_group = group;
                out.push(z);
            }
        }
    }
    out
}

/// A slice's positions: `points` are onsets in `[0, frames)` ascending, and each slice runs to the
/// next. A point too near the one before it is dropped.
pub fn slices(points: &[i64], start: u64, end: u64, min_frames: u64) -> Vec<(u64, u64)> {
    let mut at = vec![start];
    for &p in points {
        let p = p.max(0) as u64 + start;
        if p > *at.last().unwrap() + min_frames && p + min_frames < end {
            at.push(p);
        }
    }
    at.iter().enumerate().map(|(i, &s)| (s, at.get(i + 1).copied().unwrap_or(end))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> NewFile {
        NewFile { zone: Zone::new(1, name.into(), format!("{name}.wav"), 60), info_root: None, name: parse_name(name) }
    }

    #[test]
    fn names_tell_notes() {
        for (name, note) in [("Piano_C3", 60), ("Bass F#2 soft", 54), ("flute-Bb1", 46), ("Rhodes_A-1_v2", 21), ("sample 60", 60), ("eb4", 75)] {
            assert_eq!(parse_name(name).note, Some(note), "{name}");
        }
        for name in ["Kick", "Snare 2", "Take 12", "Bridge", "Add9"] {
            assert_eq!(parse_name(name).note, None, "{name}");
        }
        let i = parse_name("Piano_C3_v3_rr2");
        assert_eq!((i.note, i.layer, i.turn), (Some(60), Some(3), Some(2)));
        assert_eq!(parse_name("Violin D3 mf").layer, Some(4));
    }

    #[test]
    fn files_with_notes_split_the_keyboard() {
        let mut g = 0;
        let z = map_files(vec![file("P_C3"), file("P_C4"), file("P_G3")], None, &mut g);
        let at = |root: u8| z.iter().find(|x| x.root == root).unwrap();
        assert_eq!((at(60).key_lo, at(60).key_hi), (0, 63));
        assert_eq!((at(67).key_lo, at(67).key_hi), (64, 69));
        assert_eq!((at(72).key_lo, at(72).key_hi), (70, 127));
    }

    #[test]
    fn files_without_notes_go_up_chromatically() {
        let mut g = 0;
        let z = map_files(vec![file("Kick"), file("Snare"), file("Hat")], Some(36), &mut g);
        assert_eq!(z.iter().map(|x| (x.key_lo, x.key_hi, x.root)).collect::<Vec<_>>(), vec![(36, 36, 36), (37, 37, 37), (38, 38, 38)]);
        // One note missing is enough to go chromatic.
        let z = map_files(vec![file("P_C3"), file("Kick")], None, &mut g);
        assert_eq!((z[0].key_lo, z[1].key_lo), (60, 61));
    }

    #[test]
    fn layers_and_turns_from_names() {
        let mut g = 0;
        let z = map_files(vec![file("P_C3_v1_rr1"), file("P_C3_v1_rr2"), file("P_C3_v2")], None, &mut g);
        assert_eq!(z.len(), 3);
        assert_eq!((z[0].vel_lo, z[0].vel_hi, z[1].vel_hi), (0, 63, 63));
        assert_eq!((z[2].vel_lo, z[2].vel_hi), (64, 127));
        assert!(z[0].rr_group > 0 && z[0].rr_group == z[1].rr_group);
        assert_eq!(z[2].rr_group, 0);
    }

    #[test]
    fn one_file_takes_the_keyboard() {
        let mut g = 0;
        let z = map_files(vec![file("Vox")], None, &mut g);
        assert_eq!((z[0].key_lo, z[0].key_hi, z[0].root), (0, 127, 60));
        let z = map_files(vec![file("Vox_A2")], None, &mut g);
        assert_eq!(z[0].root, 57);
        let z = map_files(vec![file("Vox")], Some(40), &mut g);
        assert_eq!((z[0].key_lo, z[0].key_hi, z[0].root), (40, 40, 40));
    }

    #[test]
    fn slices_run_to_the_next() {
        assert_eq!(slices(&[1000, 1010, 5000], 100, 10000, 100), vec![(100, 1100), (1100, 5100), (5100, 10000)]);
        assert_eq!(slices(&[], 0, 500, 100), vec![(0, 500)]);
    }
}
