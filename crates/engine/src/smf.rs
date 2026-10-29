//! Standard MIDI File reading and writing, written by hand (formats 0 and 1).
//!
//! Times here are in ticks; the session converts them to and from project samples through
//! the file's tempo map (on import) or the project's (on export).

use anyhow::{anyhow, bail, Result};

use crate::model::CcKind;

/// Ticks per quarter note in files Fermata writes.
pub const PPQ: u16 = 960;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmfNote {
    pub start: u64,
    pub length: u64,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmfCc {
    pub start: u64,
    pub kind: CcKind,
    pub value: u16,
}

/// One part of a file: a track, or one channel of a track that used several.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmfTrack {
    pub name: String,
    /// 0-based MIDI channel.
    pub channel: u8,
    pub notes: Vec<SmfNote>,
    pub cc: Vec<SmfCc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmfFile {
    /// Ticks per quarter note.
    pub ppq: u16,
    /// The first tempo in the file, in BPM.
    pub tempo: Option<f64>,
    /// The first time signature (numerator, denominator).
    pub signature: Option<(u32, u32)>,
    pub tracks: Vec<SmfTrack>,
    /// Every tempo change as (tick, BPM), sorted. Written instead of `tempo` when not empty.
    pub tempos: Vec<(u64, f64)>,
    /// Every time signature as (tick, numerator, denominator), sorted. Written instead of
    /// `signature` when not empty.
    pub signatures: Vec<(u64, u32, u32)>,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8> {
        let v = *self.b.get(self.i).ok_or_else(|| anyhow!("unexpected end of MIDI file"))?;
        self.i += 1;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.i + n > self.b.len() {
            bail!("unexpected end of MIDI file");
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    /// A variable-length quantity (at most four bytes).
    fn var(&mut self) -> Result<u64> {
        let mut v = 0u64;
        for _ in 0..4 {
            let b = self.u8()?;
            v = (v << 7) | (b & 0x7F) as u64;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        bail!("bad variable-length number in MIDI file")
    }
}

/// Parses a Standard MIDI File. Tracks without notes or controllers (a format 1 file's tempo
/// track, say) are left out, and a track that uses several channels comes back as one track
/// per channel.
pub fn read(bytes: &[u8]) -> Result<SmfFile> {
    let mut r = Reader { b: bytes, i: 0 };
    if r.take(4)? != b"MThd" {
        bail!("not a Standard MIDI File");
    }
    let hlen = r.u32()? as usize;
    if hlen < 6 {
        bail!("bad MIDI file header");
    }
    let format = r.u16()?;
    let _ntracks = r.u16()?;
    let division = r.u16()?;
    r.take(hlen - 6)?;
    if format > 2 {
        bail!("unsupported MIDI file format {format}");
    }
    let (ppq, smpte_tempo) = if division & 0x8000 != 0 {
        // SMPTE timing: ticks per second. Read it as quarters at 120 BPM (half a second each).
        let fps = (-((division >> 8) as i8) as i32).max(1) as u32;
        let tpf = (division & 0xFF).max(1) as u32;
        (((fps * tpf) / 2).clamp(1, u16::MAX as u32) as u16, Some(120.0))
    } else {
        (division.max(1), None)
    };

    let mut file = SmfFile { ppq, tempo: smpte_tempo, signature: None, tracks: Vec::new(), tempos: Vec::new(), signatures: Vec::new() };
    while r.i + 8 <= bytes.len() {
        let id = r.take(4)?;
        let len = r.u32()? as usize;
        let len = len.min(bytes.len() - r.i);
        let body = r.take(len)?;
        if id == b"MTrk" {
            read_track(body, &mut file)?;
        }
    }
    // A tempo track comes first, but changes can sit in any track; the first one at a tick wins.
    file.tempos.sort_by_key(|t| t.0);
    file.tempos.dedup_by_key(|t| t.0);
    file.signatures.sort_by_key(|t| t.0);
    file.signatures.dedup_by_key(|t| t.0);
    if smpte_tempo.is_some() {
        file.tempos.clear();
    }
    file.tempo = file.tempos.first().map(|t| t.1).or(file.tempo);
    file.signature = file.signatures.first().map(|s| (s.1, s.2));
    Ok(file)
}

fn read_track(body: &[u8], file: &mut SmfFile) -> Result<()> {
    let mut r = Reader { b: body, i: 0 };
    let mut tick = 0u64;
    let mut running = 0u8;
    let mut name = String::new();
    // Per channel: the parts found so far, and notes waiting for their note-off (oldest
    // first for each pitch, so overlapping repeats pair up in order).
    let mut parts: Vec<SmfTrack> = (0..16).map(|ch| SmfTrack { channel: ch, ..Default::default() }).collect();
    let mut held: Vec<Vec<(u64, u8)>> = vec![Vec::new(); 16 * 128];
    while r.i < body.len() {
        tick += r.var()?;
        let mut status = r.u8()?;
        if status < 0x80 {
            // Running status: this byte was the first data byte.
            if running == 0 {
                bail!("MIDI data without a status byte");
            }
            r.i -= 1;
            status = running;
        }
        match status {
            0xFF => {
                let kind = r.u8()?;
                let len = r.var()? as usize;
                let data = r.take(len)?;
                match kind {
                    0x03 if name.is_empty() => name = String::from_utf8_lossy(data).trim().to_string(),
                    0x51 if data.len() >= 3 => {
                        let us = ((data[0] as u32) << 16 | (data[1] as u32) << 8 | data[2] as u32).max(1);
                        file.tempos.push((tick, 60_000_000.0 / us as f64));
                    }
                    0x58 if data.len() >= 2 => {
                        file.signatures.push((tick, data[0].max(1) as u32, 1u32 << data[1].min(5)));
                    }
                    0x2F => break,
                    _ => {}
                }
            }
            0xF0 | 0xF7 => {
                let len = r.var()? as usize;
                r.take(len)?;
            }
            0x80..=0xEF => {
                running = status;
                let ch = (status & 0x0F) as usize;
                let a = r.u8()? & 0x7F;
                let b = if matches!(status & 0xF0, 0xC0 | 0xD0) { 0 } else { r.u8()? & 0x7F };
                let part = &mut parts[ch];
                match status & 0xF0 {
                    0x90 if b > 0 => held[ch * 128 + a as usize].push((tick, b)),
                    0x80 | 0x90 => {
                        let q = &mut held[ch * 128 + a as usize];
                        if !q.is_empty() {
                            let (on, vel) = q.remove(0);
                            part.notes.push(SmfNote { start: on, length: (tick - on).max(1), pitch: a, velocity: vel });
                        }
                    }
                    _ => {
                        if let Some((kind, value)) = CcKind::from_message([status, a, b]) {
                            part.cc.push(SmfCc { start: tick, kind, value });
                        }
                    }
                }
            }
            // System common / real-time bytes don't belong in files; skip them.
            _ => {}
        }
    }
    // Notes never released end with the track.
    for (i, q) in held.iter().enumerate() {
        for &(on, vel) in q {
            parts[i / 128].notes.push(SmfNote { start: on, length: (tick - on).max(1), pitch: (i % 128) as u8, velocity: vel });
        }
    }
    let used: Vec<SmfTrack> = parts.into_iter().filter(|p| !p.notes.is_empty() || !p.cc.is_empty()).collect();
    let several = used.len() > 1;
    for mut p in used {
        p.notes.sort_by_key(|n| (n.start, n.pitch));
        p.cc.sort_by_key(|c| c.start);
        p.name = if several && !name.is_empty() {
            format!("{name} (Ch {})", p.channel + 1)
        } else if several {
            format!("Channel {}", p.channel + 1)
        } else {
            name.clone()
        };
        file.tracks.push(p);
    }
    Ok(())
}

fn put_var(out: &mut Vec<u8>, mut v: u64) {
    v = v.min(0x0FFF_FFFF);
    let mut buf = [0u8; 4];
    let mut n = 0;
    loop {
        buf[n] = (v & 0x7F) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    for k in (0..n).rev() {
        out.push(buf[k] | if k > 0 { 0x80 } else { 0 });
    }
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
}

/// Appends timed events to a track body as deltas, then End of Track.
fn track_body(mut events: Vec<(u64, u8, Vec<u8>)>) -> Vec<u8> {
    // At the same tick: meta first, then note-offs, controllers, and note-ons last.
    events.sort_by_key(|e| (e.0, e.1));
    let mut body = Vec::new();
    let mut now = 0;
    for (t, _, data) in events {
        put_var(&mut body, t - now);
        now = t;
        body.extend_from_slice(&data);
    }
    put_var(&mut body, 0);
    body.extend_from_slice(&[0xFF, 0x2F, 0x00]);
    body
}

fn meta(kind: u8, data: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, kind];
    put_var(&mut v, data.len() as u64);
    v.extend_from_slice(data);
    v
}

/// Writes a format 1 file: a tempo track, then one track per part.
pub fn write(file: &SmfFile) -> Vec<u8> {
    let mut out = Vec::new();
    let mut header = Vec::new();
    header.extend_from_slice(&1u16.to_be_bytes());
    header.extend_from_slice(&((file.tracks.len() + 1) as u16).to_be_bytes());
    header.extend_from_slice(&file.ppq.max(1).to_be_bytes());
    chunk(&mut out, b"MThd", &header);

    let mut conductor = Vec::new();
    let tempos = if file.tempos.is_empty() { file.tempo.map(|t| vec![(0, t)]).unwrap_or_default() } else { file.tempos.clone() };
    for (tick, bpm) in tempos {
        let us = (60_000_000.0 / bpm.max(1.0)).round().clamp(1.0, 0xFF_FFFF as f64) as u32;
        conductor.push((tick, 0, meta(0x51, &us.to_be_bytes()[1..])));
    }
    let sigs = if file.signatures.is_empty() { file.signature.map(|(n, d)| vec![(0, n, d)]).unwrap_or_default() } else { file.signatures.clone() };
    for (tick, num, den) in sigs {
        let pow = (den.max(1) as f64).log2().round() as u8;
        conductor.push((tick, 0, meta(0x58, &[num.clamp(1, 255) as u8, pow, 24, 8])));
    }
    chunk(&mut out, b"MTrk", &track_body(conductor));

    for t in &file.tracks {
        let ch = t.channel & 0x0F;
        let mut ev: Vec<(u64, u8, Vec<u8>)> = Vec::with_capacity(t.notes.len() * 2 + t.cc.len() + 1);
        if !t.name.is_empty() {
            ev.push((0, 0, meta(0x03, t.name.as_bytes())));
        }
        for n in &t.notes {
            ev.push((n.start, 3, vec![0x90 | ch, n.pitch & 0x7F, n.velocity.clamp(1, 127)]));
            ev.push((n.start + n.length.max(1), 1, vec![0x80 | ch, n.pitch & 0x7F, 0x40]));
        }
        for c in &t.cc {
            let m = c.kind.message(c.value);
            let data = if matches!(c.kind, CcKind::Aftertouch) { vec![m[0] | ch, m[1]] } else { vec![m[0] | ch, m[1], m[2]] };
            ev.push((c.start, 2, data));
        }
        chunk(&mut out, b"MTrk", &track_body(ev));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SmfFile {
        SmfFile {
            ppq: PPQ,
            tempo: Some(97.5),
            signature: Some((3, 4)),
            tracks: vec![
                SmfTrack {
                    name: "Piano".into(),
                    channel: 0,
                    notes: vec![
                        SmfNote { start: 0, length: 960, pitch: 60, velocity: 100 },
                        SmfNote { start: 0, length: 480, pitch: 64, velocity: 80 },
                        // Same pitch straight after: off and on share a tick.
                        SmfNote { start: 960, length: 960, pitch: 60, velocity: 90 },
                        SmfNote { start: 200_000, length: 1, pitch: 127, velocity: 1 },
                    ],
                    cc: vec![
                        SmfCc { start: 0, kind: CcKind::Controller(64), value: 127 },
                        SmfCc { start: 100, kind: CcKind::PitchBend, value: 12000 },
                        SmfCc { start: 700, kind: CcKind::Aftertouch, value: 33 },
                        SmfCc { start: 1900, kind: CcKind::Controller(64), value: 0 },
                    ],
                },
                SmfTrack {
                    name: "Bass".into(),
                    channel: 1,
                    notes: vec![SmfNote { start: 480, length: 240, pitch: 36, velocity: 127 }],
                    cc: vec![],
                },
            ],
            tempos: Vec::new(),
            signatures: Vec::new(),
        }
    }

    #[test]
    fn round_trip() {
        let f = sample();
        let bytes = write(&f);
        let back = read(&bytes).unwrap();
        assert_eq!(back.ppq, PPQ);
        assert!((back.tempo.unwrap() - 97.5).abs() < 0.01);
        assert_eq!(back.signature, Some((3, 4)));
        let mut want = f.tracks.clone();
        for t in &mut want {
            t.notes.sort_by_key(|n| (n.start, n.pitch));
        }
        assert_eq!(back.tracks, want);
    }

    #[test]
    fn tempo_and_signature_changes_round_trip() {
        let mut f = sample();
        f.tempos = vec![(0, 97.5), (3840, 140.0), (7680, 60.0)];
        f.signatures = vec![(0, 3, 4), (2880, 7, 8)];
        let back = read(&write(&f)).unwrap();
        assert_eq!(back.tempos.len(), 3);
        for (a, b) in back.tempos.iter().zip(&f.tempos) {
            assert_eq!(a.0, b.0);
            assert!((a.1 - b.1).abs() < 0.01, "{a:?} {b:?}");
        }
        assert_eq!(back.signatures, f.signatures);
        assert!((back.tempo.unwrap() - 97.5).abs() < 0.01);
        assert_eq!(back.signature, Some((3, 4)));
    }

    #[test]
    fn reads_running_status_and_format_0_channels() {
        // Format 0, 96 ppq: note-ons with running status, note-off as velocity 0, two channels.
        let mut trk = vec![
            0x00, 0xFF, 0x03, 0x04, b'S', b'o', b'n', b'g', // name
            0x00, 0x90, 60, 100, // ch1 on
            0x00, 64, 90, // running status: ch1 on
            0x60, 60, 0, // running status, vel 0 = off, after 96 ticks
            0x00, 64, 0, //
            0x00, 0xC1, 5, // program change on ch2 (one data byte), ignored
            0x00, 0x91, 40, 70, // ch2 on
            0x30, 0xB1, 1, 99, // ch2 mod wheel
            0x30, 0x81, 40, 0, // ch2 off
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let mut bytes = b"MThd".to_vec();
        bytes.extend_from_slice(&6u32.to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 1, 0, 96]);
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(trk.len() as u32).to_be_bytes());
        bytes.append(&mut trk);
        let f = read(&bytes).unwrap();
        assert_eq!(f.ppq, 96);
        assert_eq!(f.tracks.len(), 2);
        assert_eq!(f.tracks[0].name, "Song (Ch 1)");
        assert_eq!(f.tracks[0].notes, vec![SmfNote { start: 0, length: 96, pitch: 60, velocity: 100 }, SmfNote { start: 0, length: 96, pitch: 64, velocity: 90 }]);
        assert_eq!(f.tracks[1].channel, 1);
        assert_eq!(f.tracks[1].notes, vec![SmfNote { start: 96, length: 96, pitch: 40, velocity: 70 }]);
        assert_eq!(f.tracks[1].cc, vec![SmfCc { start: 144, kind: CcKind::Controller(1), value: 99 }]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(read(b"RIFF....").is_err());
        assert!(read(b"MThd\0\0\0\x06\0\x01\0\x01").is_err());
    }

    #[test]
    fn var_lengths() {
        for v in [0u64, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1F_FFFF, 0x0FFF_FFFF] {
            let mut b = Vec::new();
            put_var(&mut b, v);
            assert_eq!(Reader { b: &b, i: 0 }.var().unwrap(), v);
        }
    }
}
