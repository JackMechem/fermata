//! Reading SFZ files: a useful subset, so free libraries work.
//!
//! **Read:** `<control>` (`default_path`, `note_offset`, `octave_offset`), `<global>`, `<master>`,
//! `<group>`, `<region>`, `#define`, `#include`. `sample`, `key`, `lokey`, `hikey`, `lovel`,
//! `hivel`, `pitch_keycenter`, `tune` (or `pitch`), `transpose`, `volume`, `pan`, `offset`, `end`,
//! `direction`, `loop_mode`, `loop_start`, `loop_end`, `seq_length`, `seq_position` and
//! `ampeg_attack`, `ampeg_decay`, `ampeg_sustain`, `ampeg_release`. A WAV's own loop and root note
//! are used where the file doesn't say.
//!
//! **Not read:** everything else. Each such opcode is ignored and listed in [`SfzFile::ignored`]
//! with how many times it came up, and shown to the user. Regions that only sound on something
//! this doesn't do (`trigger=release`, generators like `*sine`) are left out and counted too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use super::sample::wav_info;
use super::state::Zone;

/// One region to load, with the sample it plays by its path on disk.
#[derive(Debug, Clone)]
pub struct SfzRegion {
    pub path: PathBuf,
    /// A zone with everything set but its `file`.
    pub zone: Zone,
}

#[derive(Debug, Default)]
pub struct SfzFile {
    pub regions: Vec<SfzRegion>,
    /// Opcodes this doesn't do, with how many times each came up.
    pub ignored: BTreeMap<String, usize>,
    /// Regions left out, with why.
    pub left_out: Vec<String>,
    /// Samples named that aren't on disk.
    pub missing: Vec<String>,
}

type Opcodes = BTreeMap<String, String>;

/// SFZ's note names: C4 is 60, unlike the rest of Fermata's C3.
fn sfz_note(v: &str) -> Option<i32> {
    if let Ok(n) = v.trim().parse::<i32>() {
        return Some(n);
    }
    let v = v.trim().to_ascii_lowercase();
    let mut c = v.chars();
    let base = match c.next()? {
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
    let (acc, rest) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if rest.len() > 1 && rest.starts_with('b') {
        (-1, &rest[1..])
    } else {
        (0, rest)
    };
    let oct: i32 = rest.parse().ok()?;
    Some((oct + 1) * 12 + base + acc)
}

/// Text with `//` comments gone, `#include`s read in and `#define`s applied.
fn preprocess(path: &Path, depth: usize, defines: &mut Vec<(String, String)>, out: &mut String) -> Result<()> {
    if depth > 8 {
        return Err(anyhow!("{} includes itself", path.display()));
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    // Libraries are often Windows-1252; a lossy read keeps the ASCII that matters.
    let text = String::from_utf8_lossy(&bytes);
    let dir = path.parent().unwrap_or(Path::new("."));
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("#include") {
            let name = rest.trim().trim_matches('"').replace('\\', "/");
            let inc = resolve(dir, &name).unwrap_or_else(|| dir.join(&name));
            preprocess(&inc, depth + 1, defines, out)?;
            continue;
        }
        if let Some(rest) = line.strip_prefix("#define") {
            let mut parts = rest.split_whitespace();
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                defines.push((k.to_string(), v.to_string()));
                // Longest first, so $KEY doesn't replace part of $KEYS.
                defines.sort_by_key(|d| std::cmp::Reverse(d.0.len()));
            }
            continue;
        }
        let mut l = line.to_string();
        for (k, v) in defines.iter() {
            if l.contains(k.as_str()) {
                l = l.replace(k.as_str(), v);
            }
        }
        out.push_str(&l);
        out.push('\n');
    }
    Ok(())
}

/// Finds a path under `dir`, matching each part's case loosely, as libraries made on Windows
/// expect.
fn resolve(dir: &Path, rel: &str) -> Option<PathBuf> {
    let direct = dir.join(rel);
    if direct.exists() {
        return Some(direct);
    }
    let mut at = dir.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        if part == "." {
            continue;
        }
        if part == ".." {
            at.pop();
            continue;
        }
        let exact = at.join(part);
        if exact.exists() {
            at = exact;
            continue;
        }
        let low = part.to_lowercase();
        let found = std::fs::read_dir(&at).ok()?.flatten().find(|e| e.file_name().to_string_lossy().to_lowercase() == low)?;
        at = found.path();
    }
    Some(at)
}

const READ: &[&str] = &[
    "sample", "key", "lokey", "hikey", "lovel", "hivel", "pitch_keycenter", "tune", "pitch", "transpose", "volume", "pan", "offset", "end", "direction", "loop_mode", "loopmode", "loop_start", "loopstart", "loop_end", "loopend", "seq_length", "seq_position",
    "ampeg_attack", "ampeg_decay", "ampeg_sustain", "ampeg_release", "default_path", "note_offset", "octave_offset", "trigger",
    // Names and comments in the file, which change nothing.
    "group_label", "global_label", "master_label", "region_label",
];

/// A header, or an opcode and its value.
type Token = (Option<String>, Option<(String, String)>);

/// Splits a line into headers and opcodes. A value runs to the next `word=` or `<`, so sample
/// names can have spaces in them.
fn tokens(line: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'<' {
            let Some(end) = line[i..].find('>') else { break };
            out.push((Some(line[i + 1..i + end].trim().to_ascii_lowercase()), None));
            i += end + 1;
        } else {
            let Some(eq) = line[i..].find('=') else { break };
            let key = line[i..i + eq].trim().to_ascii_lowercase();
            let vstart = i + eq + 1;
            // The next word followed by '=', or '<'.
            let mut vend = line.len();
            let mut j = vstart;
            while j < b.len() {
                if b[j] == b'<' {
                    vend = j;
                    break;
                }
                if b[j].is_ascii_whitespace() {
                    let w = j + 1;
                    let mut k = w;
                    while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                        k += 1;
                    }
                    if k > w && k < b.len() && b[k] == b'=' {
                        vend = j;
                        break;
                    }
                }
                j += 1;
            }
            out.push((None, Some((key, line[vstart..vend].trim().to_string()))));
            i = vend;
        }
    }
    out
}

/// Reads an SFZ file. Samples are found but not decoded here.
pub fn read(path: &Path) -> Result<SfzFile> {
    let mut text = String::new();
    preprocess(path, 0, &mut Vec::new(), &mut text)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut file = SfzFile::default();
    let (mut control, mut global, mut master, mut group) = (Opcodes::new(), Opcodes::new(), Opcodes::new(), Opcodes::new());
    let mut region: Option<Opcodes> = None;
    let mut section = String::new();
    let finish = |region: &mut Option<Opcodes>, control: &Opcodes, global: &Opcodes, master: &Opcodes, group: &Opcodes, file: &mut SfzFile, id: &mut u64| {
        if let Some(r) = region.take() {
            let mut all = global.clone();
            all.extend(master.clone());
            all.extend(group.clone());
            all.extend(r);
            make_region(all, control, dir, file, id);
        }
    };
    let mut id = 1;
    for line in text.lines() {
        for (header, op) in tokens(line) {
            if let Some(h) = header {
                finish(&mut region, &control, &global, &master, &group, &mut file, &mut id);
                match h.as_str() {
                    "global" => {
                        global.clear();
                        master.clear();
                        group.clear();
                    }
                    "master" => {
                        master.clear();
                        group.clear();
                    }
                    "group" => group.clear(),
                    "region" => region = Some(Opcodes::new()),
                    "control" => {}
                    other => {
                        *file.ignored.entry(format!("<{other}>")).or_default() += 1;
                    }
                }
                section = h;
            } else if let Some((k, v)) = op {
                if !READ.contains(&k.as_str()) {
                    *file.ignored.entry(k.clone()).or_default() += 1;
                }
                let map = match section.as_str() {
                    "control" => &mut control,
                    "global" => &mut global,
                    "master" => &mut master,
                    "group" => &mut group,
                    "region" => region.get_or_insert_with(Opcodes::new),
                    _ => continue,
                };
                map.insert(k, v);
            }
        }
    }
    finish(&mut region, &control, &global, &master, &group, &mut file, &mut id);
    assign_round_robin(&mut file);
    if file.regions.is_empty() && file.missing.is_empty() && file.left_out.is_empty() {
        return Err(anyhow!("{} has no regions", path.display()));
    }
    Ok(file)
}

/// What SFZ's sequence counters do: regions sharing a place on the keyboard and a `seq_length` take
/// turns in `seq_position` order. Here each such set is a round-robin group.
fn assign_round_robin(file: &mut SfzFile) {
    type Place = (u8, u8, u8, u8, u16);
    let mut groups: Vec<Place> = Vec::new();
    let mut seqs: Vec<(usize, u16)> = Vec::new();
    for (i, r) in file.regions.iter_mut().enumerate() {
        let (len, pos) = (r.zone.rr_group >> 8, r.zone.rr_group & 0xff);
        r.zone.rr_group = 0;
        if len > 1 {
            seqs.push((i, pos));
            let place = (r.zone.key_lo, r.zone.key_hi, r.zone.vel_lo, r.zone.vel_hi, len);
            let g = groups.iter().position(|p| *p == place).unwrap_or_else(|| {
                groups.push(place);
                groups.len() - 1
            });
            r.zone.rr_group = (g + 1).min(super::map::MAX_RR_GROUPS - 1) as u16;
        }
    }
    // A group's zones take their turns in zone order, so put them in sequence order.
    file.regions.sort_by_key(|r| r.zone.id);
    let mut order: Vec<(u16, u16, usize)> = seqs.iter().map(|&(i, pos)| (file.regions[i].zone.rr_group, pos, i)).collect();
    order.sort();
    let positions: Vec<usize> = {
        let mut p: Vec<usize> = seqs.iter().map(|s| s.0).collect();
        p.sort_unstable();
        p
    };
    let taken: Vec<SfzRegion> = order.iter().map(|o| file.regions[o.2].clone()).collect();
    for (slot, r) in positions.iter().zip(taken) {
        file.regions[*slot] = r;
    }
}

fn make_region(ops: Opcodes, control: &Opcodes, dir: &Path, file: &mut SfzFile, id: &mut u64) {
    let num = |k: &str| ops.get(k).and_then(|v| v.trim().parse::<f64>().ok());
    let Some(sample) = ops.get("sample") else {
        file.left_out.push("a region with no sample".into());
        return;
    };
    if sample.starts_with('*') {
        file.left_out.push(format!("a region on the generator {sample}"));
        return;
    }
    if let Some(t) = ops.get("trigger").filter(|t| *t != "attack") {
        file.left_out.push(format!("a region with trigger={t}"));
        return;
    }
    let base = control.get("default_path").map(|p| p.replace('\\', "/")).unwrap_or_default();
    let rel = format!("{base}{}", sample.replace('\\', "/"));
    let Some(path) = resolve(dir, &rel).filter(|p| p.exists()) else {
        if !file.missing.contains(&rel) {
            file.missing.push(rel);
        }
        return;
    };
    // `note_offset` moves the keys and `octave_offset` an octave at a time.
    let shift = control.get("note_offset").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0) + 12 * control.get("octave_offset").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
    let note = |k: &str| ops.get(k).and_then(|v| sfz_note(v)).map(|n| n + shift);
    let key = note("key");
    let lo = note("lokey").or(key).unwrap_or(0);
    let hi = note("hikey").or(key).unwrap_or(127);
    if lo < 0 || hi < 0 {
        file.left_out.push(format!("a region of {sample} on no keys"));
        return;
    }
    let info = wav_info(&path);
    let centre = note("pitch_keycenter").or(key).or(info.root.map(|r| r as i32)).unwrap_or(60);
    let transpose = num("transpose").unwrap_or(0.0);
    let tune = num("tune").or(num("pitch")).unwrap_or(0.0);
    let name = Path::new(&sample.replace('\\', "/")).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| sample.clone());
    let mut z = Zone::new(*id, name, String::new(), (centre - transpose as i32).clamp(0, 127) as u8);
    *id += 1;
    z.key_lo = lo.clamp(0, 127) as u8;
    z.key_hi = hi.clamp(0, 127) as u8;
    z.vel_lo = num("lovel").unwrap_or(0.0).clamp(0.0, 127.0) as u8;
    z.vel_hi = num("hivel").unwrap_or(127.0).clamp(0.0, 127.0) as u8;
    z.fine = tune as f32 + info.fine_cents * if num("tune").is_some() { 0.0 } else { 1.0 };
    z.gain_db = num("volume").unwrap_or(0.0) as f32;
    z.pan = (num("pan").unwrap_or(0.0) / 100.0) as f32;
    z.start = num("offset").unwrap_or(0.0).max(0.0) as u64;
    z.end = num("end").map(|e| e.max(0.0) as u64 + 1).unwrap_or(0);
    z.reverse = ops.get("direction").is_some_and(|d| d == "reverse");
    let mode = ops.get("loop_mode").or(ops.get("loopmode")).map(String::as_str);
    let looped = match mode {
        Some("no_loop") => false,
        Some("one_shot") => {
            z.one_shot = true;
            false
        }
        Some("loop_sustain") => {
            *file.ignored.entry("loop_mode=loop_sustain (loops on through the release)".into()).or_default() += 1;
            true
        }
        Some(_) => true,
        // Unsaid, a file's own loop decides.
        None => info.loop_points.is_some(),
    };
    if looped {
        let ls = num("loop_start").or(num("loopstart")).map(|v| v.max(0.0) as u64).or(info.loop_points.map(|l| l.0));
        let le = num("loop_end").or(num("loopend")).map(|v| v.max(0.0) as u64 + 1).or(info.loop_points.map(|l| l.1));
        z.loop_on = true;
        z.loop_start = ls.unwrap_or(0);
        z.loop_end = le.unwrap_or(0);
        // A short crossfade, so a kernel crossing the seam stays on real audio.
        z.xfade = 64;
    }
    // Only stated envelopes override the instrument's. SFZ's own defaults: no attack or decay,
    // full sustain, a release of 1 ms.
    if ["ampeg_attack", "ampeg_decay", "ampeg_sustain", "ampeg_release"].iter().any(|k| ops.contains_key(*k)) {
        z.env = Some([num("ampeg_attack").unwrap_or(0.0) as f32, num("ampeg_decay").unwrap_or(0.0) as f32, (num("ampeg_sustain").unwrap_or(100.0) / 100.0) as f32, num("ampeg_release").unwrap_or(0.001) as f32]);
    }
    // Carried in `rr_group` until the whole file is read: length above, position below.
    let (len, pos) = (num("seq_length").unwrap_or(1.0) as u16, num("seq_position").unwrap_or(1.0) as u16);
    z.rr_group = (len.min(255) << 8) | pos.min(255);
    file.regions.push(SfzRegion { path, zone: z });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path) {
        crate::decode::write_wav(path, 48000, &[vec![0.1; 1000]]).unwrap();
    }

    #[test]
    fn reads_regions_groups_and_lists_what_it_ignores() {
        let dir = std::env::temp_dir().join(format!("fermata-sfz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Samples")).unwrap();
        for n in ["a.wav", "b c.wav", "d.wav", "e1.wav", "e2.wav"] {
            write_wav(&dir.join("Samples").join(n));
        }
        let sfz = dir.join("t.sfz");
        std::fs::write(
            &sfz,
            "// comment\n#define $K 60\n<control> default_path=samples/ note_offset=0\n<global> ampeg_release=0.5 volume=-3 cutoff=1000\n\
             <group> lovel=0 hivel=63 tune=10\n<region> sample=a.wav key=$K\n<region> sample=b c.wav lokey=c5 hikey=e5 pitch_keycenter=d5 pan=-50\n\
             <group> lovel=64 hivel=127 loop_mode=loop_continuous loop_start=100 loop_end=899\n<region> sample=d.wav key=60 offset=10 end=949 transpose=2\n\
             <group> key=40 seq_length=2\n<region> seq_position=2 sample=e2.wav\n<region> seq_position=1 sample=e1.wav\n\
             <region> sample=nothing.wav key=50\n<region> sample=a.wav key=41 trigger=release\n<region> sample=*sine key=42\n",
        )
        .unwrap();
        let f = read(&sfz).unwrap();
        assert_eq!(f.regions.len(), 5);
        let a = &f.regions[0].zone;
        assert_eq!((a.key_lo, a.key_hi, a.root, a.vel_hi, a.fine, a.gain_db), (60, 60, 60, 63, 10.0, -3.0));
        assert_eq!(a.env, Some([0.0, 0.0, 1.0, 0.5]));
        let b = &f.regions[1];
        assert!(b.path.ends_with("b c.wav"));
        assert_eq!((b.zone.key_lo, b.zone.key_hi, b.zone.root, b.zone.pan), (72, 76, 74, -0.5));
        let d = &f.regions[2].zone;
        assert!(d.loop_on);
        assert_eq!((d.loop_start, d.loop_end, d.start, d.end, d.root, d.vel_lo), (100, 900, 10, 950, 58, 64));
        // Turns in seq_position order.
        assert!(f.regions[3].path.ends_with("e1.wav") && f.regions[4].path.ends_with("e2.wav"));
        assert!(f.regions[3].zone.rr_group > 0 && f.regions[3].zone.rr_group == f.regions[4].zone.rr_group);
        assert_eq!(f.ignored.get("cutoff"), Some(&1));
        assert_eq!(f.missing, vec!["samples/nothing.wav".to_string()]);
        assert_eq!(f.left_out.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn note_names_are_sfz_s_own() {
        assert_eq!(sfz_note("c4"), Some(60));
        assert_eq!(sfz_note("60"), Some(60));
        assert_eq!(sfz_note("f#3"), Some(54));
        assert_eq!(sfz_note("eb4"), Some(63));
        assert_eq!(sfz_note("c-1"), Some(0));
        assert_eq!(sfz_note("zz"), None);
    }
}
